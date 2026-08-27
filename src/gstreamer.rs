extern crate gstreamer as gst;
extern crate gstreamer_audio as gst_audio;
extern crate gstreamer_pbutils as gst_pbutils;
extern crate gstreamer_video as gst_video;

use gst::{glib, MessageView, Pipeline};
use anyhow::{Error, anyhow};
use derive_more::{Display, Error};
use poise::serenity_prelude::futures::StreamExt;

use tokio::{sync::Mutex as TokioMutex};
use url::Url;

use std::{collections::VecDeque, fmt::Debug, future::{Future}, path::Path, pin::Pin, sync::{Arc, Mutex}};
use gst_pbutils::{prelude::*, ElementPropertiesMapItem};


use uuid::Uuid;
use tracing::{error, info};

/// Type alias for the stop function callback
type StopFn = Option<Arc<TokioMutex<Pin<Box<dyn Future<Output = bool> + Send>>>>>;

/// Buffer size for uridecodebin in bytes (10 MB)
const BUFFER_SIZE: i32 = 10 * 1024 * 1024;
/// Video encoder bitrate in kbps
const VIDEO_BITRATE: u32 = 3000;
/// Video encoder quantizer value (lower = better quality, larger file)
const VIDEO_QUANTIZER: u32 = 21;

#[derive(Debug, Display, Error)]
#[display("Received error from {src}: {error} (debug: {debug:?})")]
struct ErrorMessage {
    src: glib::GString,
    error: glib::Error,
    debug: Option<glib::GString>,
}

#[derive(Clone, Debug, glib::Boxed)]
#[boxed_type(name = "ErrorValue")]
struct ErrorValue(Arc<Mutex<Option<Error>>>);

fn get_value_or_error<T>(option: Option<T>, error: &str) -> Result<T, Error> {
    option.ok_or_else(|| anyhow!("{}", error))
}

pub(crate) struct PipelineBundle {
    pipeline: gst::Pipeline,
    audio_selector: gst::Element,
    text_selector: gst::Element,
    subtitleoverlay: gst::Element,
    audio_selector_pads: Arc<Mutex<Vec<gst::Pad>>>,
    text_selector_pads: Arc<Mutex<Vec<gst::Pad>>>,
}

#[derive(Clone)]
pub(crate) struct QueueItem {
    display_name: String,
    uri: Url,
    stop_fn: StopFn,
    id: Uuid,
}

impl QueueItem {
    pub fn new(display_name: String, uri: Url, stop_fn: StopFn) -> Self {
        QueueItem {
            display_name,
            uri,
            id: Uuid::new_v4(),
            stop_fn,
        }
    }

    pub fn name(&self) -> String {
        self.display_name.clone()
    }

    pub fn uri(&self) -> Url {
        self.uri.clone()
    }

    pub fn id(&self) -> Uuid {
        self.id
    }


    #[allow(clippy::let_and_return)]
    pub async fn run_stop_fn(&self) -> bool {
        match &self.stop_fn {
            Some(func) => {
                let func = func.clone();
                let res = func.lock().await.as_mut().await;
                res
            },
            None => false,
        }
    }

}

pub(crate) struct PlayQueue {
    pipeline: gst::Pipeline,
    uris: VecDeque<QueueItem>,
    current_item: Option<QueueItem>,
    audio_selector: gst::Element,
    text_selector: gst::Element,
    subtitleoverlay: gst::Element,
    audio_selector_pads: Arc<Mutex<Vec<gst::Pad>>>,
    text_selector_pads: Arc<Mutex<Vec<gst::Pad>>>,
}

impl PlayQueue {
    pub fn new(rtmp_host: &str) -> Result<Self, Error> {
        let bundle = get_rtmp_pipeline(rtmp_host)?;
        Ok(Self {
            pipeline: bundle.pipeline,
            uris: VecDeque::new(),
            current_item: None,
            audio_selector: bundle.audio_selector,
            text_selector: bundle.text_selector,
            subtitleoverlay: bundle.subtitleoverlay,
            audio_selector_pads: bundle.audio_selector_pads,
            text_selector_pads: bundle.text_selector_pads,
        })
    }

    pub async fn add_eos_watch(play_queue: &Arc<tokio::sync::Mutex<Self>>) {
        let pipeline = {
            let playqueue = play_queue.lock().await;
            playqueue.pipeline.clone()
        };

        let bus = match pipeline.bus() {
            Some(bus) => bus,
            None => {
                error!("Failed to get pipeline bus for EOS watch");
                return;
            }
        };
        let playqueue_clone = Arc::clone(play_queue);

        let mut messages = bus.stream();

        while let Some(msg) = messages.next().await {
            match msg.view() {
                MessageView::Eos(..) => {
                    match playqueue_clone.lock().await.skip_video().await {
                        Ok(_) => (),
                        Err(e) => error!("{}", e)
                    };
                }
                MessageView::Error(err) => {
                    error!(
                        "Pipeline error from {:?}: {} ({:?})",
                        err.src().map(|s| s.path_string()),
                        err.error(),
                        err.debug()
                    );
                    match playqueue_clone.lock().await.stop_playback().await {
                        Ok(_) => (),
                        Err(e) => error!("Failed to stop pipeline after error: {}", e)
                    };
                }
                _ => ()
            }
        }
    }

    // Function to add a URI to the queue
    pub fn add_uri(&mut self, uri: String, display_name: String, stop_fn: StopFn) -> Result<QueueItem, Error> {
        let queue_uri: String = if uri.starts_with('/') {
            let path = Path::new(&uri);
            Url::from_file_path(path)
                .map_err(|_| anyhow!("Failed to convert file path to URL: {}", uri))?
                .to_string()
        } else {
            uri
        };
        let parsed_url = Url::parse(&queue_uri)
            .map_err(|e| anyhow!("Failed to parse URI '{}': {}", queue_uri, e))?;
        let queue_item = QueueItem::new(display_name, parsed_url, stop_fn);
        self.uris.push_back(queue_item.clone());
        Ok(queue_item)
    }

    // Function to remove a URI from the queue
    pub fn remove_uri(&mut self, id: &Uuid) -> Result<(), Error> {
        self.uris.retain(|u| u.id != *id);
        Ok(())
    }

    pub fn get_queue_items(&self) -> Vec<QueueItem> {
        self.uris.clone().into()
    }

    pub fn get_current_item(&self) -> Option<QueueItem> {
        self.current_item.clone()
    }

    fn queue_next_item(&mut self) -> Result<Option<QueueItem>, Error> {
        if let Some(uri) = self.uris.pop_front() {
            match set_source_uri(&self.pipeline, uri.uri().as_str()) {
                Ok(_) => {
                    self.current_item = Some(uri)
                },
                Err(e) => {
                    self.uris.push_front(uri);
                    error!("Failed to queue item {}", e);
                    return Err(anyhow!("failed to queue item: {}", e))
                }
            }
        } else {
            return Err(anyhow!("no more items left in the queue"));
        };
        Ok(self.current_item.clone())
    }

    // Function to start playback
    pub async fn start_playback(&mut self) -> Result<Option<QueueItem>, Error> {
        match self.pipeline.current_state() {
            gst::State::Null => {
                match self.queue_next_item() {
                    Ok(i) => {
                        start_pipeline(&self.pipeline)?;
                        return Ok(i)
                    }
                    Err(e) =>  return Err(e)
                }
            }
            gst::State::Paused => {
                start_pipeline(&self.pipeline)?;
            }
            _ => {
            }
        }
        Ok(self.current_item.clone())
    }

    fn reset_track_pads(&mut self) {
        {
            let mut audio_pads = self.audio_selector_pads.lock().unwrap();
            for pad in audio_pads.drain(..) {
                self.audio_selector.release_request_pad(&pad);
            }
        }
        {
            let mut text_pads = self.text_selector_pads.lock().unwrap();
            for pad in text_pads.drain(..) {
                self.text_selector.release_request_pad(&pad);
            }
        }
        self.subtitleoverlay.set_property("silent", true);
    }

    pub async fn stop_playback(&mut self) -> Result<(), Error> {
        match self.pipeline.current_state() {
            gst::State::Playing|gst::State::Paused|gst::State::Ready => {
                self.reset_track_pads();
                // Capture the result but do cleanup first — if stop_pipeline fails we still
                // want current_item cleared so PlayQueue's state stays consistent.
                let stop_result = stop_pipeline(&self.pipeline);
                if let Some(i) = &self.current_item {
                    let _ = i.run_stop_fn().await;
                }
                self.current_item = None;
                stop_result?;
            }
            _ => {
            }
        }

        Ok(())
    }

    pub async fn pause_playback(&mut self) -> Result<(), Error> {
        match self.pipeline.current_state() {
            gst::State::Playing => {
                pause_pipeline(&self.pipeline)?;
            }
            _ => {
                return Err(anyhow!("video is not currently playing"))
            }
        }

        Ok(())
    }

    pub async fn skip_video(&mut self) -> Result<(), Error> {
        match self.stop_playback().await {
            Ok(_) => {
            }
            Err(e) => {
                return Err(e)
            }
        }
        self.start_playback().await?;
        Ok(())
    }

    pub async fn seek_video(&mut self, seek_seconds: i64) -> Result<u64, Error> {
        match seek_pipeline(&self.pipeline, seek_seconds) {
            Ok(pos) => {
                Ok(pos)
            }
            Err(e) => {
                Err(e)
            }
        }
    }

    /// Returns the list of available audio tracks as human-readable names.
    /// Empty if no audio tracks have been discovered yet (pipeline stopped or audio-less source).
    pub fn get_audio_tracks(&self) -> Vec<String> {
        let pads = self.audio_selector_pads.lock().unwrap();
        pads.iter().enumerate().map(|(i, _)| format!("Audio {}", i + 1)).collect()
    }

    /// Returns available subtitle tracks. Index 0 is always "No Subtitles".
    /// Indices 1..N correspond to discovered subtitle streams.
    pub fn get_text_tracks(&self) -> Vec<String> {
        let pads = self.text_selector_pads.lock().unwrap();
        let mut v = vec!["No Subtitles".to_string()];
        v.extend((0..pads.len()).map(|i| format!("Subtitle {}", i + 1)));
        v
    }

    /// Switch to the audio track at the given index (0-based into get_audio_tracks()).
    pub fn select_audio_track(&self, index: usize) -> Result<(), Error> {
        let pad = {
            let pads = self.audio_selector_pads.lock().unwrap();
            pads.get(index)
                .ok_or_else(|| anyhow!("audio track index {} out of range (have {})", index, pads.len()))?
                .clone()
        };
        self.audio_selector.set_property("active-pad", &pad);
        Ok(())
    }

    /// Select a subtitle track. index 0 = disable subtitles; index 1..N = subtitle track N-1.
    /// Links text_selector → subtitleoverlay the first time a subtitle track is chosen.
    pub fn select_text_track(&self, index: usize) -> Result<(), Error> {
        if index == 0 {
            self.subtitleoverlay.set_property("silent", true);
            return Ok(());
        }
        let track_index = index - 1;
        // Clone the pad so we can drop the lock before calling into GStreamer.
        let pad = {
            let pads = self.text_selector_pads.lock().unwrap();
            pads.get(track_index)
                .ok_or_else(|| anyhow!("subtitle track index {} out of range (have {})", track_index, pads.len()))?
                .clone()
        };

        // Lazily link text_selector.src → subtitleoverlay.subtitle_sink the first time.
        // Must be done BEFORE setting active-pad so that the STREAM_START/CAPS/SEGMENT
        // events emitted by input-selector flow through to subtitleoverlay immediately.
        // If active-pad is set while src is unlinked, those events go nowhere and are
        // not re-sent when the link is subsequently made — subtitleoverlay never gets
        // SEGMENT, can't sync subtitles, and silently discards them on first enable.
        let text_sel_src = get_value_or_error(
            self.text_selector.static_pad("src"),
            "failed to get text_selector src pad",
        )?;
        if !text_sel_src.is_linked() {
            let subtitle_sink = get_value_or_error(
                self.subtitleoverlay.static_pad("subtitle_sink"),
                "failed to get subtitleoverlay subtitle_sink pad",
            )?;
            text_sel_src.link(&subtitle_sink)?;
        }

        // Set active-pad after the link is in place so stream events reach subtitleoverlay.
        self.text_selector.set_property("active-pad", &pad);
        self.subtitleoverlay.set_property("silent", false);
        Ok(())
    }

    /// Returns the 0-based index of the currently active audio track, or None if no track is active.
    pub fn get_current_audio_track_index(&self) -> Option<usize> {
        let active: Option<gst::Pad> = self.audio_selector.property("active-pad");
        let active = active?;
        let pads = self.audio_selector_pads.lock().unwrap();
        pads.iter().position(|p| p == &active)
    }

    /// Returns the index of the currently active subtitle track using the same indexing as
    /// get_text_tracks(): 0 = subtitles disabled, 1..N = subtitle track N-1.
    pub fn get_current_text_track_index(&self) -> usize {
        let silent: bool = self.subtitleoverlay.property("silent");
        if silent {
            return 0;
        }
        let active: Option<gst::Pad> = self.text_selector.property("active-pad");
        let active = match active {
            Some(p) => p,
            None => return 0,
        };
        let pads = self.text_selector_pads.lock().unwrap();
        pads.iter().position(|p| p == &active)
            .map(|i| i + 1) // +1 because index 0 = "No Subtitles"
            .unwrap_or(0)
    }

    // More functions for controlling playback and handling EOS, etc.
}


fn configure_encodebin_rtmp(encodebin: &gst::Element) {
    // To tell the encodebin what we want it to produce, we create an EncodingProfile
    // https://gstreamer.freedesktop.org/data/doc/gstreamer/head/gst-plugins-base-libs/html/GstEncodingProfile.html
    // This profile consists of information about the contained audio and video formats
    // as well as the container format we want everything to be combined into.

    let audiocaps = gst_audio::AudioCapsBuilder::for_encoding("audio/mpeg").channels(2).rate_range(1000..100000)
        .field("mpegversion", 1).field("layer", 3).build();
    let audio_profile =
        gst_pbutils::EncodingAudioProfile::builder(&audiocaps)
            .presence(0)
            .build();


    let encoder_props = gst_pbutils::ElementProperties::builder_map().item(
        ElementPropertiesMapItem::builder("x264enc")
            .field("pass", 5)
            .field("quantizer", VIDEO_QUANTIZER)
            .field("bitrate", VIDEO_BITRATE)
            .build()
    ).build();
    let videocaps = gst_video::VideoCapsBuilder::for_encoding("video/x-h264").build();
    let video_profile =
        gst_pbutils::EncodingVideoProfile::builder(&videocaps)
            .presence(0)
            .variable_framerate(true)
            .element_properties(encoder_props)
            .preset_name("x264enc")
            .build();

    let contianer_props = gst_pbutils::ElementProperties::builder_general().field("streamable", true).build();
    let container_profile = gst_pbutils::EncodingContainerProfile::builder(
        &gst::Caps::builder("video/x-flv").build(),
    )
    .name("container")
    .add_profile(video_profile)
    .add_profile(audio_profile)
    .element_properties(contianer_props)
    .build();

    // Finally, apply the EncodingProfile onto our encodebin element.
    encodebin.set_property("profile", &container_profile);
}

fn get_string_property(element: &gst::Element, property_name: &str) -> Result<String, Error> {
    element.property_value(property_name)
        .get::<String>()
        .map_err(|_| anyhow!(format!("Property '{}' is not a string or does not exist", property_name)))
}

pub(crate) fn start_pipeline(pipeline: &Pipeline) -> Result<String, Error> {
    if pipeline.current_state() == gst::State::Playing {
        return Err(anyhow!("stream is already playing"))
    }
    let src_element = get_value_or_error(pipeline.by_name("src"), "unable to get source element from pipeline")?;
    let set_uri = get_string_property(&src_element, "uri")?.clone();
    if pipeline.current_state() != gst::State::Paused {
        pipeline.set_state(gst::State::Ready)?;
    }
    pipeline.set_state(gst::State::Playing)?;
    Ok(set_uri)
}

pub(crate) fn seek_pipeline(pipeline: &Pipeline, seek_seconds: i64) -> Result<u64, Error> {
    if pipeline.current_state() != gst::State::Playing {
        return Err(anyhow!("cannot seek on non-playing stream"))
    }
    let src_element = get_value_or_error(pipeline.by_name("src"), "unable to get source element from pipeline")?;

    let current_pos_ct = get_value_or_error(src_element.query_position::<gst::ClockTime>(), "unable to get current position")?;
    let max_pos_ct = get_value_or_error(src_element.query_duration::<gst::ClockTime>(), "unable to get max position")?;
    info!("current position {}s", current_pos_ct.seconds());
    let new_pos = if seek_seconds.is_negative() {
        if current_pos_ct.seconds() > seek_seconds.wrapping_abs() as u64 {
            current_pos_ct.seconds() - seek_seconds.wrapping_abs() as u64
        } else {
            0
        }
    } else {
        let next_pos = current_pos_ct.seconds() + seek_seconds.wrapping_abs() as u64;
        if next_pos >= max_pos_ct.seconds() && max_pos_ct.seconds() > 0 {
            max_pos_ct.seconds()
        } else {
            next_pos
        }
    };
    let seek_flags = gst::SeekFlags::FLUSH;
    info!("setting position to {}", new_pos);

    src_element.seek_simple(seek_flags, gst::ClockTime::from_seconds(new_pos))?;

    Ok(new_pos)
}

pub(crate) fn stop_pipeline(pipeline: &Pipeline) -> Result<(), Error> {
    // set_state(Ready) flushes queued buffers but may fail when elements are in error state
    // (e.g. broken RTMP socket means flush events can't propagate downstream). Ignore the
    // result and always proceed to Null, which forces all elements to release resources.
    let _ = pipeline.set_state(gst::State::Ready);
    pipeline.set_state(gst::State::Null)?;
    Ok(())
}

pub(crate) fn pause_pipeline(pipeline: &Pipeline) -> Result<(), Error> {
    if pipeline.current_state() != gst::State::Playing {
        return Err(anyhow!("stream is not playing. unable to pause"))
    }
    pipeline.set_state(gst::State::Paused)?;
    Ok(())
}

pub(crate) fn set_source_uri(pipeline: &Pipeline, uri_path: &str) -> Result<(), Error> {
    let src_element = get_value_or_error(pipeline.by_name("src"), "unable to get source element from pipeline")?;
    src_element.set_property_from_str("uri", uri_path);
    info!("set url to {}", uri_path);
    Ok(())
}

pub(crate) fn get_rtmp_pipeline(rtmp_host: &str) -> Result<PipelineBundle, Error>  {

    gst::init()?;

    let audio_queue = gst::ElementFactory::make("queue").build()?;

    // Small frame limit so subtitle state changes (silent toggle, track switch) are visible
    // within ~400ms without needing a flush seek. Byte/time limits disabled; only frame count
    // matters. x264enc encodes well above real-time so 10 frames of back-pressure tolerance is fine.
    let video_queue = gst::ElementFactory::make("queue")
        .property("max-size-buffers", 10u32)
        .property("max-size-bytes", 0u32)
        .property("max-size-time", 0u64)
        .build()?;
    let video_convert = gst::ElementFactory::make("videoconvert").build()?;
    let video_scale = gst::ElementFactory::make("videoscale").build()?;
    let audio_convert = gst::ElementFactory::make("audioconvert").build()?;
    let audio_resample = gst::ElementFactory::make("audioresample").build()?;
    let suboverlay = gst::ElementFactory::make("subtitleoverlay").build()?;
    let audio_selector = gst::ElementFactory::make("input-selector").build()?;
    let text_selector = gst::ElementFactory::make("input-selector").build()?;

    let encodebin = gst::ElementFactory::make("encodebin").build()?;
    let sink = gst::ElementFactory::make("rtmpsink").property("location", rtmp_host).build()?;


    let pipeline = gst::Pipeline::default();
    pipeline.add_many([&encodebin, &sink])?;
    pipeline.add_many([&video_queue, &audio_queue])?;
    pipeline.add_many([&video_convert, &video_scale, &audio_convert, &audio_resample])?;
    pipeline.add(&suboverlay)?;
    pipeline.add_many([&audio_selector, &text_selector])?;

    gst::Element::link_many([&encodebin, &sink])?;
    gst::Element::link_many([&suboverlay, &video_queue, &video_convert, &video_scale])?;
    // audio_selector feeds into audio_queue (text_selector is linked lazily on first subtitle selection)
    gst::Element::link_many([&audio_selector, &audio_queue, &audio_convert, &audio_resample])?;

    configure_encodebin_rtmp(&encodebin);

    let sink_audio_encode_pad = get_value_or_error(encodebin.request_pad_simple("audio_%u"), "unable to get audio sink from encodebin")?;
    let sink_video_encode_pad = get_value_or_error(encodebin.request_pad_simple("video_%u"), "unable to get video sink from encodebin")?;

    // link the end of the chain to the encoder
    let audio_src_pad = get_value_or_error(audio_resample.static_pad("src"), "failed to get audio_resample src pad")?;
    let video_src_pad = get_value_or_error(video_scale.static_pad("src"), "failed to get video_scale src pad")?;
    audio_src_pad.link(&sink_audio_encode_pad)?;
    video_src_pad.link(&sink_video_encode_pad)?;

    let video_sink_real = get_value_or_error(suboverlay.static_pad("video_sink"), "failed to get video sink for uridecode")?;

    // Subtitles start disabled; text_selector → subtitleoverlay link is created lazily on first use
    suboverlay.set_property("silent", true);

    let uridecode = gst::ElementFactory::make("uridecodebin")
        .name("src")
        .property("force-sw-decoders", true)
        .property("use-buffering", true)
        .property("buffer-size", BUFFER_SIZE)
        .build()?;

    pipeline.add(&uridecode)?;

    let audio_selector_pads: Arc<Mutex<Vec<gst::Pad>>> = Arc::new(Mutex::new(Vec::new()));
    let text_selector_pads: Arc<Mutex<Vec<gst::Pad>>> = Arc::new(Mutex::new(Vec::new()));

    let audio_sel_c = audio_selector.clone();
    let text_sel_c = text_selector.clone();
    let audio_pads_c = Arc::clone(&audio_selector_pads);
    let text_pads_c = Arc::clone(&text_selector_pads);

    uridecode.connect_pad_added(move |_src, src_pad| {
        let pad_caps = match src_pad.current_caps() {
            Some(caps) => caps,
            None => {
                error!("Failed to get current caps for pad");
                return;
            }
        };
        let pad_struct = match pad_caps.structure(0) {
            Some(s) => s,
            None => {
                error!("Failed to get structure from pad caps");
                return;
            }
        };
        let pad_type = pad_struct.name();

        if pad_type.starts_with("video/x-raw") {
            if video_sink_real.is_linked() {
                info!("video sink is already linked!");
                return;
            }
            if let Err(e) = src_pad.link(&video_sink_real) {
                error!("Failed to link video pad: {:?}", e);
            }
            return;
        }

        if pad_type.starts_with("audio/x-raw") {
            let selector_sink = match audio_sel_c.request_pad_simple("sink_%u") {
                Some(p) => p,
                None => {
                    error!("failed to request audio selector sink pad");
                    return;
                }
            };
            if let Err(e) = src_pad.link(&selector_sink) {
                error!("Failed to link audio pad to selector: {:?}", e);
                return;
            }
            let mut pads = audio_pads_c.lock().unwrap();
            if pads.is_empty() {
                // First audio pad: make it active so audio plays immediately
                audio_sel_c.set_property("active-pad", &selector_sink);
            }
            pads.push(selector_sink);
            return;
        }

        if pad_type.starts_with("text/x-raw") {
            let selector_sink = match text_sel_c.request_pad_simple("sink_%u") {
                Some(p) => p,
                None => {
                    error!("failed to request text selector sink pad");
                    return;
                }
            };
            if let Err(e) = src_pad.link(&selector_sink) {
                error!("Failed to link text pad to selector: {:?}", e);
                return;
            }
            let mut pads = text_pads_c.lock().unwrap();
            // Do not activate or un-silence here; user opts in via select_text_track()
            pads.push(selector_sink);
        }
    });

    Ok(PipelineBundle {
        pipeline,
        audio_selector,
        text_selector,
        subtitleoverlay: suboverlay,
        audio_selector_pads,
        text_selector_pads,
    })
}
