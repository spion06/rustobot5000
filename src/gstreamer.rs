extern crate gstreamer as gst;
extern crate gstreamer_audio as gst_audio;
extern crate gstreamer_pbutils as gst_pbutils;
extern crate gstreamer_video as gst_video;
use gst::{glib, MessageView, Pipeline};
use anyhow::{Error, anyhow};
use derive_more::{Display, Error};
use poise::serenity_prelude::futures::StreamExt;

use url::Url;

use std::{collections::VecDeque, fmt::Debug, path::Path, sync::{Arc, Mutex}};
use gst_pbutils::{prelude::*, ElementPropertiesMapItem};


use uuid::Uuid;
use tracing::{error, info};




#[derive(Debug, Display, Error)]
#[display(fmt = "Received error from {src}: {error} (debug: {debug:?})")]
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

#[derive(Clone)]
pub(crate) struct QueueItem {
    display_name: String,
    uri: Url,
    id: Uuid,
}

impl QueueItem {
    pub fn new(display_name: String, uri: Url) -> Self {
        QueueItem {
            display_name: display_name,
            uri: uri,
            id: Uuid::new_v4(),
        }
    }

    pub fn name(&self) -> String {
        self.display_name.clone()
    }
    
    pub fn uri(&self) -> Url {
        self.uri.clone()
    }

    pub fn id(&self) -> Uuid {
        self.id.clone()
    }
    
}

pub(crate) struct PlayQueue {
    pipeline: gst::Pipeline,
    uris: VecDeque<QueueItem>,
    current_item: Option<QueueItem>,
}

impl PlayQueue {
    pub fn new(rtmp_host: &str) -> Result<Self, Error> {
        let pipeline = get_rtmp_pipeline(rtmp_host)?;
        // Initialize and add necessary elements to the pipeline

        Ok(
            Self {
               pipeline,
               uris: VecDeque::new(),
               current_item: None,
            }
        )
    }

    pub async fn add_eos_watch(play_queue: &Arc<tokio::sync::Mutex<Self>>) {
        let pipeline = {
            let playqueue = play_queue.lock().await;
            playqueue.pipeline.clone()
        };

        let bus = pipeline.bus().unwrap();
        let playqueue_clone = Arc::clone(play_queue);

        let mut messages = bus.stream();

        while let Some(msg) = messages.next().await {
            match msg.view() {
                MessageView::Eos(..) => {
                    match playqueue_clone.lock().await.skip_video() {
                        Ok(_) => (),
                        Err(e) => error!("{}", e)
                    };
                    ()
                },
                _ => (),
            }
        }
    }

    // Function to add a URI to the queue
    pub fn add_uri(&mut self, uri: String, display_name: String) -> Result<QueueItem, Error> {
        let queue_uri: String;
        if uri.starts_with("/") {
            let path = Path::new(&uri);
            queue_uri = Url::from_file_path(path).unwrap().to_string();
        } else {
            queue_uri = uri;
        }
        let queue_item = QueueItem::new(display_name, Url::parse(&queue_uri).unwrap());
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
    pub fn start_playback(&mut self) -> Result<Option<QueueItem>, Error> {
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

    pub fn stop_playback(&mut self) -> Result<(), Error> {
        match self.pipeline.current_state() {
            gst::State::Playing|gst::State::Paused|gst::State::Ready => {
                stop_pipeline(&self.pipeline)?;
                self.current_item = None;
            }
            _ => {
            }
        }

        Ok(())
    }

    pub fn pause_playback(&mut self) -> Result<(), Error> {
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

    pub fn skip_video(&mut self) -> Result<(), Error> {
        match self.stop_playback() {
            Ok(_) => {
            }
            Err(e) => {
                return Err(e)
            }
        }
        self.start_playback()?;
        Ok(())
    }

    pub fn seek_video(&mut self, seek_seconds: i64) -> Result<(), Error> {
        match seek_pipeline(&self.pipeline, seek_seconds) {
            Ok(_) => {
            }
            Err(e) => {
                return Err(e)
            }
        }
        Ok(())
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
            .field("quantizer", 21)
            .field("bitrate", 3000)
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

pub(crate) fn seek_pipeline(pipeline: &Pipeline, seek_seconds: i64) -> Result<(), Error> {
    if pipeline.current_state() != gst::State::Playing {
        return Err(anyhow!("cannot seek on non-playing stream"))
    }
    let src_element = get_value_or_error(pipeline.by_name("src"), "unable to get source element from pipeline")?;

    let current_pos_ct = get_value_or_error(src_element.query_position::<gst::ClockTime>(), "unable to get current position")?;
    info!("current position {}s", current_pos_ct.seconds());
    let new_pos = if seek_seconds.is_negative() {
        current_pos_ct.seconds() - seek_seconds.wrapping_abs() as u64
    } else {
        current_pos_ct.seconds() + seek_seconds.wrapping_abs() as u64
    };
    let seek_flags = gst::SeekFlags::FLUSH;
    info!("setting position to {}", new_pos);

    src_element.seek_simple(seek_flags, gst::ClockTime::from_seconds(new_pos))?;

    return Ok(())
}

pub(crate) fn stop_pipeline(pipeline: &Pipeline) -> Result<(), Error> {
    pipeline.set_state(gst::State::Ready)?;
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

pub(crate) fn get_rtmp_pipeline(rtmp_host: &str) -> Result<Pipeline, Error>  {

    gst::init()?;

    let audio_queue = gst::ElementFactory::make("queue").build()?;

    let video_queue = gst::ElementFactory::make("queue").build()?;
    let video_convert = gst::ElementFactory::make("videoconvert").build()?;
    let video_scale = gst::ElementFactory::make("videoscale").build()?;
    let audio_convert = gst::ElementFactory::make("audioconvert").build()?;
    let audio_resample = gst::ElementFactory::make("audioresample").build()?;
    let suboverlay = gst::ElementFactory::make("subtitleoverlay").build()?;

    let encodebin = gst::ElementFactory::make("encodebin").build()?;
    let sink = gst::ElementFactory::make("rtmpsink").property("location", &rtmp_host).build()?;


    let pipeline = gst::Pipeline::default();
    pipeline.add_many([&encodebin, &sink])?;
    pipeline.add_many([&video_queue, &audio_queue])?;
    pipeline.add_many([&video_convert, &video_scale, &audio_convert, &audio_resample])?;
    pipeline.add(&suboverlay)?;

    gst::Element::link_many([&encodebin, &sink])?;
    gst::Element::link_many([&suboverlay, &video_queue, &video_convert, &video_scale])?;
    gst::Element::link_many([&audio_queue, &audio_convert, &audio_resample])?;

    configure_encodebin_rtmp(&encodebin);

    let sink_audio_encode_pad = get_value_or_error(encodebin.request_pad_simple("audio_%u"), "unable to get audio sink from encodebin")?;
    let sink_video_encode_pad = get_value_or_error(encodebin.request_pad_simple("video_%u"), "unable to get video sink from encodebin")?;

    // link the end of the chain to the encoder
    audio_resample.static_pad("src").unwrap().link(&sink_audio_encode_pad)?;
    video_scale.static_pad("src").unwrap().link(&sink_video_encode_pad)?;

    let video_sink_real = get_value_or_error(suboverlay.static_pad("video_sink"), "failed to get video sink for uridecode")?;
    let subtitle_sink_real = get_value_or_error(suboverlay.static_pad("subtitle_sink"), "filed to get subtitle sink for uridecode")?;
    let audio_sink_real = get_value_or_error(audio_queue.static_pad("sink"), "failed to get audio sink for uridecode")?;

    let uridecode = gst::ElementFactory::make("uridecodebin")
        .name("src")
        .property("force-sw-decoders", true)
        .property("use-buffering", true)
        .property("buffer-size", 10 * 1024 * 1024)
        .build()?;

    pipeline.add(&uridecode)?;


    uridecode.connect_pad_added(move |_src, src_pad| {
        let pad_caps = src_pad.current_caps().unwrap();
        let pad_struct = pad_caps.structure(0).unwrap();
        let pad_type = pad_struct.name();
        if pad_type.starts_with("video/x-raw") {
            if video_sink_real.is_linked() {
                println!("video sink is already linked!");
                return;
            }
            src_pad.link(&video_sink_real).unwrap();
        }
        if pad_type.starts_with("audio/x-raw") {
            if audio_sink_real.is_linked() {
                println!("audio sink is already linked!");
                return;
            }
            src_pad.link(&audio_sink_real).unwrap();
        }
        if pad_type.starts_with("text/x-raw") {
            if subtitle_sink_real.is_linked() {
                println!("subtitle sink is already linked!");
                return;
            }
            src_pad.link(&subtitle_sink_real).unwrap();
        }
    });

    Ok(pipeline)
}
 