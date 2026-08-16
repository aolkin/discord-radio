use crate::state::Data;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use futures_util::stream::Stream;
use ogg::{PacketWriteEndInfo, PacketWriter};
use serenity::model::id::GuildId;
use std::io::Cursor;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

/// Creates the OpusHead header packet for Ogg/Opus streams
/// Based on RFC 7845 Section 5.1
fn create_opus_head() -> Vec<u8> {
    let mut head = Vec::new();
    head.extend_from_slice(b"OpusHead"); // Magic signature
    head.push(1); // Version
    head.push(2); // Channel count (stereo)
    head.extend_from_slice(&312u16.to_le_bytes()); // Pre-skip (312 samples at 48kHz)
    head.extend_from_slice(&48000u32.to_le_bytes()); // Input sample rate
    head.extend_from_slice(&0i16.to_le_bytes()); // Output gain (0 dB)
    head.push(0); // Channel mapping family (0 = mono or stereo)
    head
}

/// Creates the OpusTags header packet for Ogg/Opus streams
/// Based on RFC 7845 Section 5.2
fn create_opus_tags() -> Vec<u8> {
    let mut tags = Vec::new();
    tags.extend_from_slice(b"OpusTags"); // Magic signature

    let vendor = b"radio-bot-rs";
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);

    tags.extend_from_slice(&0u32.to_le_bytes()); // No user comments
    tags
}

/// Stream that receives pre-encoded Opus packets and wraps them in Ogg containers
pub struct OpusOggStream<'a> {
    receiver: BroadcastStream<Vec<u8>>,
    packet_writer: Option<PacketWriter<'a, Cursor<Vec<u8>>>>,
    granule_position: u64,
    stream_serial: u32,
    packet_count: u64,
}

impl<'a> OpusOggStream<'a> {
    pub fn new(receiver: broadcast::Receiver<Vec<u8>>) -> Self {
        Self {
            receiver: BroadcastStream::new(receiver),
            packet_writer: None,
            granule_position: 0,
            stream_serial: rand::random(),
            packet_count: 0,
        }
    }

    fn initialize_stream(&mut self) -> Result<Vec<u8>, std::io::Error> {
        let buffer = Cursor::new(Vec::new());
        let mut writer = PacketWriter::new(buffer);

        // Write OpusHead packet (first packet, beginning of stream)
        let opus_head = create_opus_head();
        writer.write_packet(
            opus_head,
            self.stream_serial,
            PacketWriteEndInfo::EndPage,
            0, // Granule position for header is 0
        )?;

        // Write OpusTags packet (second packet)
        let opus_tags = create_opus_tags();
        writer.write_packet(
            opus_tags,
            self.stream_serial,
            PacketWriteEndInfo::EndPage,
            0, // Granule position for tags is 0
        )?;

        let initial_data = writer.into_inner().into_inner();
        self.packet_writer = Some(PacketWriter::new(Cursor::new(Vec::new())));

        Ok(initial_data)
    }

    fn write_opus_packet(&mut self, opus_packet: Vec<u8>) -> Result<Vec<u8>, std::io::Error> {
        // Each Opus packet represents 20ms at 48kHz = 960 samples
        const SAMPLES_PER_PACKET: u64 = 960;

        self.granule_position += SAMPLES_PER_PACKET;
        self.packet_count += 1;

        let writer = self.packet_writer.as_mut().unwrap();

        // Flush pages more frequently for lower latency streaming
        // End page every packet to minimize buffering
        writer.write_packet(
            opus_packet,
            self.stream_serial,
            PacketWriteEndInfo::EndPage,
            self.granule_position,
        )?;

        let buffer = std::mem::replace(writer.inner_mut(), Cursor::new(Vec::new()));

        Ok(buffer.into_inner())
    }
}

impl<'a> Stream for OpusOggStream<'a> {
    type Item = Result<Vec<u8>, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Initialize stream on first poll
        if self.packet_writer.is_none() {
            match self.initialize_stream() {
                Ok(header_data) => return Poll::Ready(Some(Ok(header_data))),
                Err(e) => return Poll::Ready(Some(Err(e))),
            }
        }

        // Poll for next Opus packet
        match Pin::new(&mut self.receiver).poll_next(cx) {
            Poll::Ready(Some(Ok(opus_packet))) => match self.write_opus_packet(opus_packet) {
                Ok(ogg_data) => Poll::Ready(Some(Ok(ogg_data))),
                Err(e) => Poll::Ready(Some(Err(e))),
            },
            Poll::Ready(Some(Err(e))) => {
                tracing::warn!("Broadcast receive error (lagged): {:?}", e);
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

    // Create the streaming response with Ogg-wrapped Opus packets
    let stream = OpusOggStream::new(receiver);

    // Return streaming response with appropriate headers
    let body = axum::body::Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "audio/ogg; codecs=opus")
        .header("Cache-Control", "no-cache, no-store, must-revalidate")
        .header("Pragma", "no-cache")
        .header("Expires", "0")
        .header("X-Content-Type-Options", "nosniff")
        .header("Connection", "keep-alive")
        .body(body)
        .unwrap())
}
