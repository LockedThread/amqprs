use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info};

use super::{BufWriter, OutgoingMessage};

pub(crate) struct WriterHandler {
    stream: BufWriter,
    /// receiver half to forward outgoing messages from AMQ connection/channel to server
    outgoing_rx: mpsc::Receiver<OutgoingMessage>,
    /// listener of shutdown signal
    shutdown: broadcast::Receiver<()>,
}

impl WriterHandler {
    pub fn new(
        stream: BufWriter,
        outgoing_rx: mpsc::Receiver<OutgoingMessage>,
        shutdown: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            stream,
            outgoing_rx,
            shutdown,
        }
    }

    pub async fn run_until_shutdown(mut self) {
        loop {
            tokio::select! {
                biased;

                channel_frame = self.outgoing_rx.recv() => {
                    let (channel_id, frame) = match channel_frame {
                        None => break,
                        Some(v) => v,
                    };
                    if let Err(err) = self.stream.write_frame(channel_id, frame).await {
                        error!("failed to send frame over network, cause: {}!", err);
                        break;
                    }
                }
                _ = self.shutdown.recv() => {
                    info!("received shutdown notification.");
                    break;
                }
                else => {
                    break;
                }
            }
        }
        debug!("shutdown!");
    }
}
