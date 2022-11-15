use std::collections::BTreeMap;

use amqp_serde::types::{AmqpChannelId, ShortUint};
use tokio::sync::{
    broadcast,
    mpsc::{Receiver, Sender},
    oneshot,
};
use tracing::{debug, error, info};

use crate::frame::{
    Close, CloseChannel, CloseChannelOk, CloseOk, Frame, MethodHeader, CONN_DEFAULT_CHANNEL,
};

use super::{
    channel_id_repo::ChannelIdRepository, BufReader, ChannelResource, ConnManagementCommand, Error,
    OutgoingMessage, IncomingMessage,
};

/////////////////////////////////////////////////////////////////////////////
struct ChannelManager {
    /// channel id allocator and manager
    channel_id_repo: ChannelIdRepository,

    /// channel resource registery store
    resource: BTreeMap<AmqpChannelId, ChannelResource>,
}

impl ChannelManager {
    fn new(channel_max: ShortUint) -> Self {
        Self {
            channel_id_repo: ChannelIdRepository::new(channel_max),
            resource: BTreeMap::new(),
        }
    }
    /// Insert channel resource, when open a new channel
    fn insert_resource(
        &mut self,
        channel_id: Option<AmqpChannelId>,
        resource: ChannelResource,
    ) -> Option<AmqpChannelId> {
        let id = match channel_id {
            // reserve channel id as requested
            Some(id) => {
                if self.channel_id_repo.reserve(&id) {
                    match self.resource.insert(id, resource) {
                        Some(_old) => unreachable!("Implementation error"),
                        None => id,
                    }
                } else {
                    // fail to reserve the id
                    return None;
                }
            }
            // allocate a channel id
            None => {
                // allocate id never fail
                let id = self.channel_id_repo.allocate();
                match self.resource.insert(id, resource) {
                    Some(_old) => unreachable!("Implementation error"),
                    None => id,
                }
            }
        };

        Some(id)
    }

    /// remove channel resource, when channel to be closed
    fn remove_resource(&mut self, channel_id: &AmqpChannelId) -> Option<ChannelResource> {
        assert_eq!(
            true,
            self.channel_id_repo.release(channel_id),
            "Implementation error"
        );
        // remove responder means channel is to be  closed
        self.resource.remove(channel_id)
    }

    fn insert_responder(
        &mut self,
        channel_id: &AmqpChannelId,
        method_header: &'static MethodHeader,
        responder: oneshot::Sender<IncomingMessage>,
    ) -> Option<oneshot::Sender<IncomingMessage>> {
        self.resource
            .get_mut(channel_id)?
            .responders
            .insert(method_header, responder)
    }

    fn remove_responder(
        &mut self,
        channel_id: &AmqpChannelId,
        method_header: &'static MethodHeader,
    ) -> Option<oneshot::Sender<IncomingMessage>> {
        self.resource
            .get_mut(channel_id)?
            .responders
            .remove(method_header)
    }

    fn get_dispatcher(&self, channel_id: &AmqpChannelId) -> Option<&Sender<IncomingMessage>> {
        self.resource.get(channel_id)?.dispatcher.as_ref()
    }
}

/////////////////////////////////////////////////////////////////////////////

pub(super) struct ReaderHandler {
    stream: BufReader,

    /// sender half to forward outgoing message to `WriterHandler`
    outgoing_tx: Sender<OutgoingMessage>,

    /// receiver half to receive management command from AMQ Connection/Channel
    conn_mgmt_rx: Receiver<ConnManagementCommand>,

    channel_manager: ChannelManager,

    /// Notify WriterHandler to shutdown.
    /// If reader handler exit first, it will notify writer handler to shutdown.
    /// If writer handler exit first, socket connection will be shutdown because the writer half drop,
    /// so socket read will return, and reader handler can detect connection shutdown without separate signal.
    #[allow(dead_code /* notify shutdown just by dropping the instance */)]
    shutdown_notifier: broadcast::Sender<()>,

    to_shutdown: bool,
}

impl ReaderHandler {
    pub fn new(
        stream: BufReader,
        outgoing_tx: Sender<OutgoingMessage>,
        conn_mgmt_rx: Receiver<ConnManagementCommand>,
        channel_max: ShortUint,
        shutdown_notifier: broadcast::Sender<()>,
    ) -> Self {
        Self {
            stream,
            outgoing_tx,
            conn_mgmt_rx,
            channel_manager: ChannelManager::new(channel_max),
            shutdown_notifier,
            to_shutdown: false,
        }
    }

    async fn handle_close(
        &mut self,
        channel_id: AmqpChannelId,
        _method_header: &'static MethodHeader,
        _close: Close,
    ) -> Result<(), Error> {
        assert_eq!(CONN_DEFAULT_CHANNEL, channel_id, "must be from channel 0");

        self.to_shutdown = true;
        self.outgoing_tx
            .send((CONN_DEFAULT_CHANNEL, CloseOk::default().into_frame()))
            .await?;
        Ok(())
    }

    async fn handle_close_ok(
        &mut self,
        channel_id: AmqpChannelId,
        method_header: &'static MethodHeader,
        close_ok: CloseOk,
    ) -> Result<(), Error> {
        assert_eq!(CONN_DEFAULT_CHANNEL, channel_id, "must be from channel 0");

        self.to_shutdown = true;
        let responder = self
            .channel_manager
            .remove_responder(&channel_id, method_header)
            .ok_or_else(|| {
                Error::InternalChannelError(format!(
                    "No responder to forward frame {:?} to channel {}",
                    close_ok, channel_id
                ))
            })?;
        responder
            .send(close_ok.into_frame())
            .map_err(|response| Error::InternalChannelError(response.to_string()))?;
        Ok(())
    }

    async fn handle_close_channel(
        &mut self,
        channel_id: AmqpChannelId,
        _method_header: &'static MethodHeader,
        _close_channel: CloseChannel,
    ) -> Result<(), Error> {
        // first, respond to server that we have received the request
        self.outgoing_tx
            .send((channel_id, CloseChannelOk::default().into_frame()))
            .await?;

        // clean up channel resource
        self.channel_manager
            .remove_resource(&channel_id)
            .ok_or_else(|| {
                Error::InternalChannelError(format!(
                    "No channel resource found for channel {}",
                    channel_id
                ))
            })?;
        Ok(())
    }

    ///
    async fn handle_close_channel_ok(
        &mut self,
        channel_id: AmqpChannelId,
        method_header: &'static MethodHeader,
        close_channel_ok: CloseChannelOk,
    ) -> Result<(), Error> {
        let responder = self
            .channel_manager
            .remove_responder(&channel_id, method_header)
            .ok_or_else(|| {
                Error::InternalChannelError(format!(
                    "No responder to forward frame {:?} to channel {}",
                    close_channel_ok, channel_id
                ))
            })?;

        responder
            .send(close_channel_ok.into_frame())
            .map_err(|response| Error::InternalChannelError(response.to_string()))?;

        // clean up channel resource
        self.channel_manager
            .remove_resource(&channel_id)
            .ok_or_else(|| {
                Error::InternalChannelError(format!(
                    "No channel resource found for channel {}",
                    channel_id
                ))
            })?;

        Ok(())
    }

    /// If OK, user can continue to handle frame
    /// If NOK, user should stop consuming frame
    /// TODO: implement as Iterator, then user do not need to care about the error
    async fn handle_frame(&mut self, channel_id: AmqpChannelId, frame: Frame) -> Result<(), Error> {
        match frame {
            // TODO: handle Blocked and Unblocked from server
            Frame::Blocked(..) => todo!(),
            Frame::Unblocked(..) => todo!(),

            // Server request to close connection
            Frame::Close(method_header, close) => {
                self.handle_close(channel_id, method_header, close).await
            }
            // Close connection response from server
            Frame::CloseOk(method_header, close_ok) => {
                self.handle_close_ok(channel_id, method_header, close_ok)
                    .await
            }
            // Server request to close channel
            Frame::CloseChannel(method_header, close_channel) => {
                self.handle_close_channel(channel_id, method_header, close_channel)
                    .await
            }
            // Close channel response from server
            Frame::CloseChannelOk(method_header, close_channel_ok) => {
                self.handle_close_channel_ok(channel_id, method_header, close_channel_ok)
                    .await
            }

            // TODO: Handle heartbeat
            Frame::HeartBeat(_) => {
                debug!("heartbeat, to be handled...");
                Ok(())
            }

            // Deliver/GetOk/Return, ContentHeader, ContentBody are delivered in sequence by server.
            Frame::Deliver(_, _)
            | Frame::GetOk(_, _)
            | Frame::GetEmpty(_, _)
            | Frame::Return(_, _)
            | Frame::ContentHeader(_)
            | Frame::ContentBody(_) => match self.channel_manager.get_dispatcher(&channel_id) {
                Some(dispatcher) => {
                    dispatcher.send(frame).await?;
                    Ok(())
                }
                None => {
                    debug!(
                        "No dispatcher registered yet for channel {}, discard frame: {}",
                        channel_id, frame
                    );
                    Ok(())
                }
            },

            // Method frames for synchronous response
            Frame::StartOk(method_header, _)
            | Frame::SecureOk(method_header, _)
            | Frame::TuneOk(method_header, _)
            | Frame::OpenOk(method_header, _)
            | Frame::UpdateSecretOk(method_header, _)
            | Frame::OpenChannelOk(method_header, _)
            | Frame::FlowOk(method_header, _)
            | Frame::RequestOk(method_header, _)
            | Frame::DeclareOk(method_header, _)
            | Frame::DeleteOk(method_header, _)
            | Frame::BindOk(method_header, _)
            | Frame::UnbindOk(method_header, _)
            | Frame::DeclareQueueOk(method_header, _)
            | Frame::BindQueueOk(method_header, _)
            | Frame::PurgeQueueOk(method_header, _)
            | Frame::DeleteQueueOk(method_header, _)
            | Frame::UnbindQueueOk(method_header, _)
            | Frame::QosOk(method_header, _)
            | Frame::ConsumeOk(method_header, _)
            | Frame::CancelOk(method_header, _)
            | Frame::RecoverOk(method_header, _)
            | Frame::SelectOk(method_header, _)
            | Frame::SelectTxOk(method_header, _)
            | Frame::CommitOk(method_header, _)
            | Frame::RollbackOk(method_header, _) => {
                // handle synchronous response
                match self
                    .channel_manager
                    .remove_responder(&channel_id, method_header)
                {
                    Some(responder) => {
                        if let Err(response) = responder.send(frame) {
                            debug!(
                                "Failed to forward response frame {} to channel {}",
                                response, channel_id
                            );
                        }
                    }
                    None => debug!(
                        "No responder to forward frame {} to channel {}",
                        frame, channel_id
                    ),
                }

                Ok(())
            }

            // Method frames of asynchronous request
            Frame::Start(_method_header, _)
            | Frame::Secure(_method_header, _)
            | Frame::Tune(_method_header, _)
            | Frame::Open(_method_header, _)
            | Frame::UpdateSecret(_method_header, _)
            | Frame::OpenChannel(_method_header, _)
            | Frame::Flow(_method_header, _)
            | Frame::Request(_method_header, _)
            | Frame::Declare(_method_header, _)
            | Frame::Delete(_method_header, _)
            | Frame::Bind(_method_header, _)
            | Frame::Unbind(_method_header, _)
            | Frame::DeclareQueue(_method_header, _)
            | Frame::BindQueue(_method_header, _)
            | Frame::PurgeQueue(_method_header, _)
            | Frame::DeleteQueue(_method_header, _)
            | Frame::UnbindQueue(_method_header, _)
            | Frame::Qos(_method_header, _)
            | Frame::Consume(_method_header, _)
            | Frame::Cancel(_method_header, _)
            | Frame::Publish(_method_header, _)
            | Frame::Get(_method_header, _)
            | Frame::Ack(_method_header, _)
            | Frame::Reject(_method_header, _)
            | Frame::RecoverAsync(_method_header, _)
            | Frame::Recover(_method_header, _)
            | Frame::Nack(_method_header, _)
            | Frame::Select(_method_header, _)
            | Frame::SelectTx(_method_header, _)
            | Frame::Commit(_method_header, _)
            | Frame::Rollback(_method_header, _) => {
                todo!("handle asynchronous request")
            }
        }
    }

    pub async fn run_until_shutdown(mut self) {
        loop {
            tokio::select! {
                biased;

                command = self.conn_mgmt_rx.recv() => {
                    let command = match command {
                        None => break,
                        Some(v) => v,
                    };
                    match command {
                        ConnManagementCommand::RegisterChannelResource(cmd) => {
                            let id = self.channel_manager.insert_resource(cmd.channel_id, cmd.resource);
                            cmd.acker.send(id).expect("Acknowledge to command RegisterChannelResource should succeed");
                        },
                        ConnManagementCommand::RegisterResponder(cmd) => {
                            self.channel_manager.insert_responder(&cmd.channel_id, cmd.method_header, cmd.responder);
                            cmd.acker.send(()).expect("Acknowledge to command RegisterResponder should succeed");
                        },
                    }
                }

                res = self.stream.read_frame() => {
                    match res {
                        Ok((channel_id, frame)) => {
                            if let Err(err) = self.handle_frame(channel_id, frame).await {
                                error!("Failed to handle frame, cause: {} ", err);
                                break;
                            }
                            if self.to_shutdown {
                                info!("Client has requested to shutdown connection or shutdown requested by server!");
                                break;
                            }
                        },
                        Err(err) => {
                            error!("Failed to read frame, cause: {}", err);
                            break;
                        },
                    }

                }
                else => {
                    break;
                }
            }
        }

        // `self` will drop, so the `self.shutdown_notifier`
        // all tasks which have `subscribed` to `shutdown_notifier` will be notified
        info!("Shutdown ReaderHandler!");
    }
}