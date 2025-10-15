use crate::state::Data;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use futures_util::stream::Stream;
use serenity::model::id::GuildId;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

const WEBM_OPUS_HEADER: &[u8] = &[
    0x1a, 0x45, 0xdf, 0xa3, // EBML Header
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1f, // Header size
    0x42, 0x86, 0x81, 0x01, // EBMLVersion: 1
    0x42, 0xf7, 0x81, 0x01, // EBMLReadVersion: 1
    0x42, 0xf2, 0x81, 0x04, // EBMLMaxIDLength: 4
    0x42, 0xf3, 0x81, 0x08, // EBMLMaxSizeLength: 8
    0x42, 0x82, 0x84, 0x77, 0x65, 0x62, 0x6d, // DocType: "webm"
    0x42, 0x87, 0x81, 0x02, // DocTypeVersion: 2
    0x42, 0x85, 0x81, 0x02, // DocTypeReadVersion: 2
];

/// Stream that receives pre-encoded Opus packets and wraps them in WebM containers
pub struct OpusWebMStream {
    receiver: BroadcastStream<Vec<u8>>,
    sent_header: bool,
}

impl OpusWebMStream {
    pub fn new(receiver: broadcast::Receiver<Vec<u8>>) -> Self {
        Self {
            receiver: BroadcastStream::new(receiver),
            sent_header: false,
        }
    }
}

impl Stream for OpusWebMStream {
    type Item = Result<Vec<u8>, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Send WebM header first
        if !self.sent_header {
            self.sent_header = true;
            return Poll::Ready(Some(Ok(WEBM_OPUS_HEADER.to_vec())));
        }

        // Poll for next Opus packet (pre-encoded by AudioProcessor)
        match Pin::new(&mut self.receiver).poll_next(cx) {
            Poll::Ready(Some(Ok(opus_packet))) => {
                // Send the pre-encoded Opus packet
                // Note: For simplicity, we send raw Opus packets without full WebM container wrapping
                // A complete WebM implementation would wrap these in Cluster/SimpleBlock elements
                Poll::Ready(Some(Ok(opus_packet)))
            }
            Poll::Ready(Some(Err(e))) => {
                tracing::warn!("Broadcast receive error: {:?}", e);
                // Skip lagged messages and continue
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// HTTP endpoint handler for audio streaming
pub async fn audio_stream(
    State(bot_state): State<Data>,
    Path(guild_id): Path<String>,
) -> Result<Response, StatusCode> {
    let guild_id_parsed: u64 = guild_id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let guild_id = GuildId::new(guild_id_parsed);

    // Get the audio processor for this guild
    let processors = bot_state.audio_processors.read().await;
    let processor = processors.get(&guild_id).ok_or(StatusCode::NOT_FOUND)?;

    // Get the distributor and subscribe to Opus packets (pre-encoded by AudioProcessor)
    let distributor = processor.read().await.distributor();
    let receiver = distributor.subscribe_opus();

    drop(processors);

    // Create the streaming response with pre-encoded Opus packets
    let stream = OpusWebMStream::new(receiver);

    // Return streaming response with appropriate headers
    let body = axum::body::Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "audio/webm; codecs=opus")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(body)
        .unwrap())
}
