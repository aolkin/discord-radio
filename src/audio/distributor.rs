use tokio::sync::broadcast;

const CHANNEL_CAPACITY: usize = 16; // Buffer up to 16 chunks (320ms at 20ms per chunk)

/// Distributes PCM audio frames to multiple consumers using a non-blocking broadcast channel.
/// This allows simultaneous streaming to Songbird (Discord) and web clients.
pub struct AudioDistributor {
    pcm_tx: broadcast::Sender<Vec<[f32; 2]>>,
    opus_tx: broadcast::Sender<Vec<u8>>,
}

impl AudioDistributor {
    /// Create a new audio distributor
    pub fn new() -> Self {
        let (pcm_tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        let (opus_tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { pcm_tx, opus_tx }
    }

    /// Broadcast a chunk of PCM frames to all subscribers
    /// Returns the number of receivers that got the data
    pub fn broadcast_pcm(&self, frames: Vec<[f32; 2]>) -> usize {
        // broadcast will fail if there are no active receivers, which is fine
        self.pcm_tx.send(frames).unwrap_or(0)
    }

    /// Broadcast an Opus packet to all web streaming subscribers
    /// Returns the number of receivers that got the data
    pub fn broadcast_opus(&self, packet: Vec<u8>) -> usize {
        self.opus_tx.send(packet).unwrap_or(0)
    }

    /// Subscribe to receive PCM audio frames (for Songbird or other PCM consumers)
    /// Returns a receiver that will get copies of all broadcasted frames
    #[allow(dead_code)]
    pub fn subscribe_pcm(&self) -> broadcast::Receiver<Vec<[f32; 2]>> {
        self.pcm_tx.subscribe()
    }

    /// Subscribe to receive Opus packets (for web streaming)
    /// Returns a receiver that will get copies of all broadcasted Opus packets
    pub fn subscribe_opus(&self) -> broadcast::Receiver<Vec<u8>> {
        self.opus_tx.subscribe()
    }

    /// Get the number of active subscribers
    #[allow(dead_code)]
    pub fn receiver_count(&self) -> usize {
        self.pcm_tx.receiver_count() + self.opus_tx.receiver_count()
    }
}

impl Default for AudioDistributor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
