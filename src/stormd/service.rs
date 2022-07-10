// Storm node providing distributed storage & messaging for lightning network.
//
// Written in 2022 by
//     Dr. Maxim Orlovsky <orlovsky@lnp-bp.org>
//
// Copyright (C) 2022 by LNP/BP Standards Association, Switzerland.
//
// You should have received a copy of the MIT License along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

use std::collections::BTreeSet;
use std::ops::Deref;

use internet2::addr::NodeId;
use internet2::{Unmarshall, ZmqSocketType};
use lnp2p::bifrost;
use lnp2p::bifrost::{BifrostApp, Messages as LnMsg};
use lnp_rpc::ClientId;
use microservices::error::BootstrapError;
use microservices::esb;
use microservices::esb::{EndpointList, Error};
use microservices::node::TryService;
use storm::p2p::{Messages, STORM_P2P_UNMARSHALLER};
use storm::StormApp;
use storm_ext::{ExtMsg, StormExtMsg};
use storm_rpc::{RpcMsg, ServiceId};

use crate::bus::{BusMsg, CtlMsg, Endpoints, Responder, ServiceBus};
use crate::stormd::Daemon;
use crate::{Config, DaemonError, LaunchError};

pub fn run(config: Config<super::Config>) -> Result<(), BootstrapError<LaunchError>> {
    let msg_endpoint = config.msg_endpoint.clone();
    let rpc_endpoint = config.rpc_endpoint.clone();
    let ctl_endpoint = config.ctl_endpoint.clone();
    let ext_endpoint = config.ext_endpoint.clone();
    let runtime = Runtime::init(config)?;

    debug!("Connecting to service bus {}", msg_endpoint);
    let controller = esb::Controller::with(
        map! {
            ServiceBus::Storm => esb::BusConfig::with_addr(
                ext_endpoint,
                ZmqSocketType::RouterBind,
                None,
            ),
            ServiceBus::Ctl => esb::BusConfig::with_addr(
                ctl_endpoint,
                ZmqSocketType::RouterBind,
                None,
            ),
            ServiceBus::Msg => esb::BusConfig::with_addr(
                msg_endpoint,
                ZmqSocketType::RouterConnect,
                Some(ServiceId::Lnp)
            ),
            ServiceBus::Rpc => esb::BusConfig::with_addr(
                rpc_endpoint,
                ZmqSocketType::Rep,
                None
            )
        },
        runtime,
    )
    .map_err(|_| LaunchError::BusSetupFailure)?;

    controller.run_or_panic("stormd");

    unreachable!()
}

pub struct Runtime {
    pub(super) config: Config<super::Config>,
    registered_apps: BTreeSet<StormApp>,
}

impl Runtime {
    pub fn init(config: Config<super::Config>) -> Result<Self, BootstrapError<LaunchError>> {
        // debug!("Initializing storage provider {:?}", config.storage_conf());
        // let storage = storage::FileDriver::with(config.storage_conf())?;

        info!("Stormd runtime started successfully");

        Ok(Self {
            config,
            registered_apps: empty!(),
        })
    }
}

impl Responder for Runtime {}

impl esb::Handler<ServiceBus> for Runtime {
    type Request = BusMsg;
    type Error = DaemonError;

    fn identity(&self) -> ServiceId { ServiceId::stormd() }

    fn on_ready(&mut self, _senders: &mut Endpoints) -> Result<(), Self::Error> {
        if self.config.ext.run_chat {
            info!("Starting chat daemon...");
            let config = Config::with(self.config.clone(), ());
            self.launch_daemon(Daemon::Chatd, config)?;
        }
        Ok(())
    }

    fn handle(
        &mut self,
        endpoints: &mut EndpointList<ServiceBus>,
        bus_id: ServiceBus,
        source: ServiceId,
        request: Self::Request,
    ) -> Result<(), Self::Error> {
        match (bus_id, request, source) {
            (ServiceBus::Msg, BusMsg::Bifrost(msg), ServiceId::Peer(remote_id)) => {
                self.handle_p2p(endpoints, remote_id, msg)
            }
            (ServiceBus::Ctl, BusMsg::Ctl(msg), source) => self.handle_ctl(endpoints, source, msg),
            (ServiceBus::Storm, BusMsg::Storm(msg), ServiceId::StormApp(app_id)) => {
                self.handle_app(endpoints, app_id, msg)
            }
            (ServiceBus::Rpc, BusMsg::Rpc(msg), ServiceId::Client(client_id)) => {
                self.handle_rpc(endpoints, client_id, msg)
            }
            (bus, msg, _) => Err(DaemonError::wrong_esb_msg(bus, &msg)),
        }
    }

    fn handle_err(
        &mut self,
        _endpoints: &mut EndpointList<ServiceBus>,
        _error: Error<ServiceId>,
    ) -> Result<(), Self::Error> {
        // We do nothing and do not propagate error; it's already being reported
        // with `error!` macro by the controller. If we propagate error here
        // this will make whole daemon panic
        Ok(())
    }
}

impl Runtime {
    fn handle_p2p(
        &mut self,
        endpoints: &mut Endpoints,
        remote_id: NodeId,
        message: LnMsg,
    ) -> Result<(), DaemonError> {
        if let LnMsg::Message(bifrost::Msg {
            app: BifrostApp::Storm,
            payload,
        }) = &message
        {
            let mesg = STORM_P2P_UNMARSHALLER.unmarshall(&**payload)?.deref().clone();
            match mesg.storm_ext_msg(remote_id) {
                Ok((app, storm_msg)) => self.send_ext(endpoints, Some(app), storm_msg)?,

                // Messages we process ourselves
                Err(Messages::ListApps) => {
                    self.send_p2p(
                        endpoints,
                        remote_id,
                        Messages::ActiveApps(self.registered_apps.clone()),
                    )?;
                }

                // A remote peer described list of apps. We need to report that to a client.
                Err(Messages::ActiveApps(_)) => {}

                // These should be processed by transfer service
                /*
                Messages::PullContainer(_) => {}
                Messages::PushContainer(_) => {}
                Messages::Reject(_) => {}
                Messages::PullChunk(_) => {}
                Messages::PushChunk(_) => {}
                 */
                _ => {}
            }
        } else {
            error!("Request is not supported by the RPC interface");
            return Err(DaemonError::wrong_esb_msg(ServiceBus::Rpc, &message));
        }
        Ok(())
    }

    fn handle_rpc(
        &mut self,
        endpoints: &mut Endpoints,
        client_id: ClientId,
        message: RpcMsg,
    ) -> Result<(), DaemonError> {
        match message {
            wrong_msg => {
                error!("Request is not supported by the RPC interface");
                return Err(DaemonError::wrong_esb_msg(ServiceBus::Rpc, &wrong_msg));
            }
        }
    }

    fn handle_ctl(
        &mut self,
        endpoints: &mut Endpoints,
        source: ServiceId,
        message: CtlMsg,
    ) -> Result<(), DaemonError> {
        match &message {
            CtlMsg::Hello => {
                // TODO: Process with daemon registration
            }

            wrong_msg => {
                error!("Request is not supported by the CTL interface");
                return Err(DaemonError::wrong_esb_msg(ServiceBus::Ctl, wrong_msg));
            }
        }

        Ok(())
    }

    fn handle_app(
        &mut self,
        endpoints: &mut Endpoints,
        app: StormApp,
        message: ExtMsg,
    ) -> Result<(), DaemonError> {
        match message {
            ExtMsg::RegisterApp(app_id) => {
                if app == app_id {
                    info!("Application {} is registered", app_id);
                    self.registered_apps.insert(app_id);
                } else {
                    error!(
                        "Request on application {} registration issued by a non-application \
                         daemon {}",
                        app,
                        ServiceId::StormApp(app_id)
                    );
                    return Err(DaemonError::wrong_esb_msg_source(
                        ServiceBus::Storm,
                        &message,
                        ServiceId::StormApp(app_id),
                    ));
                }
            }

            // We need to the rest of the messages to the Bifrost network
            forward => {
                self.send_p2p(endpoints, forward.remote_id(), forward.p2p_message(app))?;
            }
        }

        Ok(())
    }
}
