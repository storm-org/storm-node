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
use std::fmt::{self, Display, Formatter};

use internet2::addr::NodeId;
use microservices::rpc;
use storm::p2p::AppMsg;
use storm::{p2p, Mesg, MesgId, StormApp, Topic};
use strict_encoding::{StrictDecode, StrictEncode};

/// We need this wrapper type to be compatible with Storm Node having multiple message buses
#[derive(Clone, Debug, Display, From, Api)]
#[api(encoding = "strict")]
#[non_exhaustive]
pub(crate) enum BusMsg {
    #[api(type = 5)]
    #[display(inner)]
    #[from]
    Ext(ExtMsg),
}

impl rpc::Request for BusMsg {}

#[derive(Clone, Debug, Display, Api, From)]
#[derive(NetworkEncode, NetworkDecode)]
#[api(encoding = "strict")]
#[non_exhaustive]
pub enum ExtMsg {
    /// An extension app connecting to the Storm node must first signal with this message its app
    /// id. After that storm node will be able to route messages coming from Bifrost network
    /// targeting this app.
    #[api(type = 0x0100)]
    #[display("register_app({0})")]
    RegisterApp(StormApp),

    /* TODO: Consider developing sync API like
    /// Extension request to sync topics with the remote peer.
    #[api(type = 0x0004)]
    #[display("sync_topics({0})")]
    SyncTopics(NodeId),

    SyncMessages(PeerMsg<MesgFullId>),
     */
    /// List topics known to the local Storm node.
    #[api(type = 0x0102)]
    #[display("list_topics()")]
    ListTopics(AddressedMsg<()>),

    /// Response to `ListTopics` request.
    #[api(type = 0x0103)]
    #[display("topics(...)")]
    Topics(AddressedMsg<BTreeSet<MesgId>>),

    /// Sent or received propose to create a new Storm application topic which must be accepted or
    /// not.
    #[api(type = 0x0006)]
    #[display("propose_topic(...)")]
    ProposeTopic(AddressedMsg<Topic>),

    /// A message sent from Storm node to the app extension on arrival of the new information from
    /// remote peer via Bifrost network.
    #[api(type = 0x0008)]
    #[display("post_received({0})")]
    Post(AddressedMsg<Mesg>),

    /// A message from app extension to external peer requesting certain message or a topic from a
    /// remote peer.
    #[api(type = 0x000a)]
    #[display("post_retrieve({0})")]
    Read(AddressedMsg<MesgId>),

    /// Command to the storm node to decline the topic or a message with a specific id coming from
    /// certain peer.
    #[api(type = 0x000c)]
    #[display("decline({0})")]
    Decline(AddressedMsg<MesgId>),

    /// Command to the storm node to accept the topic or a message with a specific id coming from
    /// certain peer. This also requests the node to download all the unknown containers for the
    /// topic or the message.
    #[api(type = 0x000e)]
    #[display("accept({0})")]
    Accept(AddressedMsg<MesgId>),
}

impl ExtMsg {
    pub fn remote_id(&self) -> NodeId {
        match self {
            ExtMsg::RegisterApp(_) => {
                unreachable!("ExtMsg::remote_id must not be called on ExtMsg::RegisterApp")
            }
            ExtMsg::ListTopics(AddressedMsg { remote_id, .. })
            | ExtMsg::Topics(AddressedMsg { remote_id, .. })
            | ExtMsg::ProposeTopic(AddressedMsg { remote_id, .. })
            | ExtMsg::Post(AddressedMsg { remote_id, .. })
            | ExtMsg::Read(AddressedMsg { remote_id, .. })
            | ExtMsg::Decline(AddressedMsg { remote_id, .. })
            | ExtMsg::Accept(AddressedMsg { remote_id, .. }) => *remote_id,
        }
    }

    pub fn p2p_message(self, app: StormApp) -> p2p::Messages {
        match self {
            ExtMsg::RegisterApp(_) => {
                unreachable!("ExtMsg::remote_id must not be called on ExtMsg::RegisterApp")
            }
            ExtMsg::ListTopics(AddressedMsg { data, .. }) => {
                p2p::Messages::ListTopics(AppMsg { app, data })
            }
            ExtMsg::Topics(AddressedMsg { data, .. }) => {
                p2p::Messages::AppTopics(AppMsg { app, data })
            }
            ExtMsg::ProposeTopic(AddressedMsg { data, .. }) => {
                p2p::Messages::ProposeTopic(AppMsg { app, data })
            }
            ExtMsg::Post(AddressedMsg { data, .. }) => p2p::Messages::Post(AppMsg { app, data }),
            ExtMsg::Read(AddressedMsg { data, .. }) => p2p::Messages::Read(AppMsg { app, data }),
            ExtMsg::Decline(AddressedMsg { data, .. }) => {
                p2p::Messages::Decline(AppMsg { app, data })
            }
            ExtMsg::Accept(AddressedMsg { data, .. }) => {
                p2p::Messages::Accept(AppMsg { app, data })
            }
        }
    }

    pub fn to_payload(&self) -> Vec<u8> {
        match self {
            ExtMsg::RegisterApp(_) => {
                unreachable!("ExtMsg::remote_id must not be called on ExtMsg::RegisterApp")
            }
            ExtMsg::ListTopics(AddressedMsg { data, .. }) => data.strict_serialize(),
            ExtMsg::Topics(AddressedMsg { data, .. }) => data.strict_serialize(),
            ExtMsg::ProposeTopic(AddressedMsg { data, .. }) => data.strict_serialize(),
            ExtMsg::Post(AddressedMsg { data, .. }) => data.strict_serialize(),
            ExtMsg::Read(AddressedMsg { data, .. }) => data.strict_serialize(),
            ExtMsg::Decline(AddressedMsg { data, .. }) => data.strict_serialize(),
            ExtMsg::Accept(AddressedMsg { data, .. }) => data.strict_serialize(),
        }
        .expect("extension-generated message can't be serialized as a bifrost message payload")
    }
}

#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Debug, NetworkEncode, NetworkDecode)]
pub struct AddressedMsg<T>
where T: StrictEncode + StrictDecode
{
    pub remote_id: NodeId,
    pub data: T,
}

impl<T> Display for AddressedMsg<T>
where T: Display + StrictEncode + StrictDecode
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}, {}", self.remote_id, self.data)
    }
}

impl<T> AddressedMsg<T>
where T: StrictEncode + StrictDecode
{
    pub fn with(app_msg: AppMsg<T>, remote_peer: NodeId) -> Self {
        AddressedMsg {
            remote_id: remote_peer,
            data: app_msg.data,
        }
    }
}
