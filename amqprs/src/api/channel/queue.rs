use super::{Channel, Result, ServerSpecificArguments};
use crate::frame::{
    BindQueue, BindQueueOk, DeclareQueueOk, DeleteQueue, DeleteQueueOk, PurgeQueue, PurgeQueueOk,
    UnbindQueue, UnbindQueueOk,
};
use crate::{
    api::error::Error,
    frame::{DeclareQueue, Frame},
};

#[derive(Debug, Clone)]
pub struct QueueDeclareArguments {
    pub queue: String,
    pub passive: bool,
    pub durable: bool,
    pub exclusive: bool,
    pub auto_delete: bool,
    pub no_wait: bool,
    pub arguments: ServerSpecificArguments,
}

impl QueueDeclareArguments {
    pub fn new(queue: &str) -> Self {
        Self {
            queue: queue.to_string(),
            passive: false,
            durable: false,
            exclusive: false,
            auto_delete: false,
            no_wait: false,
            arguments: ServerSpecificArguments::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueueBindArguments {
    pub queue: String,
    pub exchange: String,
    pub routing_key: String,
    pub no_wait: bool,
    pub arguments: ServerSpecificArguments,
}

impl QueueBindArguments {
    pub fn new(queue: &str, exchange: &str, routing_key: &str) -> Self {
        Self {
            queue: queue.to_string(),
            exchange: exchange.to_string(),
            routing_key: routing_key.to_string(),
            no_wait: false,
            arguments: ServerSpecificArguments::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueuePurgeArguments {
    pub queue: String,
    pub no_wait: bool,
}

impl QueuePurgeArguments {
    pub fn new(queue: &str) -> Self {
        Self {
            queue: queue.to_string(),
            no_wait: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueueDeleteArguments {
    pub queue: String,
    pub if_unused: bool,
    pub if_empty: bool,
    pub no_wait: bool,
}

impl QueueDeleteArguments {
    pub fn new(queue: &str) -> Self {
        Self {
            queue: queue.to_string(),
            if_unused: false,
            if_empty: false,
            no_wait: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueueUnbindArguments {
    pub queue: String,
    pub exchange: String,
    pub routing_key: String,
    pub arguments: ServerSpecificArguments,
}

impl QueueUnbindArguments {
    pub fn new(queue: &str, exchange: &str, routing_key: &str) -> Self {
        Self {
            queue: queue.to_string(),
            exchange: exchange.to_string(),
            routing_key: routing_key.to_string(),
            arguments: ServerSpecificArguments::new(),
        }
    }
}

/////////////////////////////////////////////////////////////////////////////
impl Channel {
    pub async fn queue_declare(&self, args: QueueDeclareArguments) -> Result<()> {
        let mut declare = DeclareQueue {
            ticket: 0,
            queue: args.queue.try_into().unwrap(),
            bits: 0,
            arguments: args.arguments.into_field_table(),
        };
        declare.set_passive(args.passive);
        declare.set_durable(args.durable);
        declare.set_exclusive(args.exclusive);
        declare.set_auto_delete(args.auto_delete);
        declare.set_no_wait(args.no_wait);
        if args.no_wait {
            self.shared
                .outgoing_tx
                .send((self.shared.channel_id, declare.into_frame()))
                .await?;
            Ok(())
        } else {
            let responder_rx = self.register_responder(DeclareQueueOk::header()).await?;
            synchronous_request!(
                self.shared.outgoing_tx,
                (self.shared.channel_id, declare.into_frame()),
                responder_rx,
                Frame::DeclareQueueOk,
                Error::ChannelUseError
            )?;
            Ok(())
        }
    }

    pub async fn queue_bind(&self, args: QueueBindArguments) -> Result<()> {
        let bind = BindQueue {
            ticket: 0,
            queue: args.queue.try_into().unwrap(),
            exchange: args.exchange.try_into().unwrap(),
            routing_key: args.routing_key.try_into().unwrap(),
            nowait: args.no_wait,
            arguments: args.arguments.into_field_table(),
        };

        if args.no_wait {
            self.shared
                .outgoing_tx
                .send((self.shared.channel_id, bind.into_frame()))
                .await?;
            Ok(())
        } else {
            let responder_rx = self.register_responder(BindQueueOk::header()).await?;

            synchronous_request!(
                self.shared.outgoing_tx,
                (self.shared.channel_id, bind.into_frame()),
                responder_rx,
                Frame::BindQueueOk,
                Error::ChannelUseError
            )?;
            Ok(())
        }
    }

    pub async fn queue_purge(&self, args: QueuePurgeArguments) -> Result<()> {
        let purge = PurgeQueue {
            ticket: 0,
            queue: args.queue.try_into().unwrap(),
            nowait: args.no_wait,
        };

        if args.no_wait {
            self.shared
                .outgoing_tx
                .send((self.shared.channel_id, purge.into_frame()))
                .await?;
            Ok(())
        } else {
            let responder_rx = self.register_responder(PurgeQueueOk::header()).await?;

            synchronous_request!(
                self.shared.outgoing_tx,
                (self.shared.channel_id, purge.into_frame()),
                responder_rx,
                Frame::PurgeQueueOk,
                Error::ChannelUseError
            )?;
            Ok(())
        }
    }

    pub async fn queue_delete(&self, args: QueueDeleteArguments) -> Result<()> {
        let mut delete = DeleteQueue {
            ticket: 0,
            queue: args.queue.try_into().unwrap(),
            bits: 0,
        };
        delete.set_if_unused(args.if_unused);
        delete.set_if_empty(args.if_empty);
        delete.set_no_wait(args.no_wait);
        if args.no_wait {
            self.shared
                .outgoing_tx
                .send((self.shared.channel_id, delete.into_frame()))
                .await?;
            Ok(())
        } else {
            let responder_rx = self.register_responder(DeleteQueueOk::header()).await?;

            synchronous_request!(
                self.shared.outgoing_tx,
                (self.shared.channel_id, delete.into_frame()),
                responder_rx,
                Frame::DeleteQueueOk,
                Error::ChannelUseError
            )?;
            Ok(())
        }
    }

    pub async fn queue_unbind(&self, args: QueueUnbindArguments) -> Result<()> {
        let unbind = UnbindQueue {
            ticket: 0,
            queue: args.queue.try_into().unwrap(),
            exchange: args.exchange.try_into().unwrap(),
            routing_key: args.routing_key.try_into().unwrap(),
            arguments: args.arguments.into_field_table(),
        };

        let responder_rx = self.register_responder(UnbindQueueOk::header()).await?;

        synchronous_request!(
            self.shared.outgoing_tx,
            (self.shared.channel_id, unbind.into_frame()),
            responder_rx,
            Frame::UnbindQueueOk,
            Error::ChannelUseError
        )?;
        Ok(())
    }
}
