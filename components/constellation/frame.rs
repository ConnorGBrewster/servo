/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use msg::constellation_msg::{FrameId, PipelineId};
use pipeline::Pipeline;
use servo_url::ServoUrl;
use std::collections::HashMap;
use std::iter::once;
use std::mem::replace;
use std::time::Instant;

/// A frame in the frame tree.
/// Each frame is the constellation's view of a browsing context.
/// Each browsing context has a session history, caused by
/// navigation and traversing the history. Each frame has its
/// current entry, plus past and future entries. The past is sorted
/// chronologically, the future is sorted reverse chronologically:
/// in particular prev.pop() is the latest past entry, and
/// next.pop() is the earliest future entry.
#[derive(Debug)]
pub struct Frame {
    /// The frame id.
    pub id: FrameId,

    /// The pipeline for the current session history entry
    pub pipeline_id: PipelineId,

    /// The currently active group of entries. All entries that share the same pipeline represent
    /// a group.
    current_group: FrameStateEntries,

    /// The past session history, ordered chronologically grouped by pipelines.
    pub past: Vec<FrameStateGroup>,

    /// The future session history, ordered reverse chronologically grouped by pipelines.
    pub future: Vec<FrameStateGroup>,
}

impl Frame {
    /// Create a new frame.
    /// Note this just creates the frame, it doesn't add it to the frame tree.
    pub fn new(id: FrameId, pipeline_id: PipelineId, url: ServoUrl) -> Frame {
        Frame {
            id: id,
            pipeline_id: pipeline_id,
            current_group: FrameStateEntries::new(url),
            past: vec!(),
            future: vec!(),
        }
    }

    /// Set the current frame entry, and push the current frame entry into the past.
    pub fn load(&mut self, pipeline_id: PipelineId, url: ServoUrl) {
        let old_entries = replace(&mut self.current_group, FrameStateEntries::new(url));
        self.past.push(FrameStateGroup {
            pipeline_id: Some(self.pipeline_id),
            frame_id: self.id,
            entries: old_entries,
        });
        self.pipeline_id = pipeline_id;
    }

    /// Set the future to be empty.
    pub fn remove_forward_entries(&mut self) -> Vec<FrameStateGroup> {
        replace(&mut self.future, vec!())
    }

    pub fn current(&self) -> HistoryEntry {
        HistoryEntry {
            pipeline_id: Some(self.pipeline_id),
            frame_id: self.id,
            url: self.current_group.current.url.clone(),
            instant: self.current_group.current.instant,
        }
    }

    pub fn instant(&self) -> Instant {
        self.current_group.current.instant
    }

    pub fn traverse_to_entry(&mut self, entry: HistoryEntry, pipeline_id: PipelineId) {
        let mut current_pipeline = Some(self.pipeline_id);
        if entry.instant > self.instant() {
            // Traversing the future
            loop {
                if self.current_group.traverse_to_entry(&entry) {
                    break;
                }
                match self.future.pop() {
                    Some(next) => {
                        // The current index does not matter as it will be search for in the next
                        // iteration.
                        let old_entries = replace(&mut self.current_group, next.entries);
                        self.past.push(FrameStateGroup {
                            pipeline_id: current_pipeline,
                            frame_id: self.id,
                            entries: old_entries,
                        });
                        current_pipeline = next.pipeline_id;
                    },
                    None => break,
                }
            }
        } else if entry.instant < self.instant() {
            // Traversing the past
            loop {
                if self.current_group.traverse_to_entry(&entry) {
                    break;
                }
                match self.past.pop() {
                    Some(prev) => {
                        // The current index does not matter as it will be search for in the next
                        // iteration.
                        let old_entries = replace(&mut self.current_group, prev.entries);
                        self.future.push(FrameStateGroup {
                            pipeline_id: current_pipeline,
                            frame_id: self.id,
                            entries: old_entries,
                        });
                        current_pipeline = prev.pipeline_id;
                    },
                    None => break,
                }
            }
        }

        debug_assert_eq!(self.instant(), entry.instant);

        self.pipeline_id = pipeline_id;
    }

    pub fn future_iter<'a>(&'a self) -> impl Iterator<Item=HistoryEntry> + 'a {
        self.current_group.future.iter().map(move |entry| {
            HistoryEntry {
                pipeline_id: Some(self.pipeline_id),
                frame_id: self.id,
                instant: entry.instant,
                url: entry.url.clone()
            }
        }).chain(self.future.iter().flat_map(|group| {
            group.entries.past.iter().chain(once(&group.entries.current))
                .chain(group.entries.future.iter()).map(move |entry| {
                    HistoryEntry {
                        pipeline_id: group.pipeline_id,
                        frame_id: group.frame_id,
                        instant: entry.instant,
                        url: entry.url.clone()
                    }
                })
        }))
    }

    pub fn past_iter<'a>(&'a self) -> impl Iterator<Item=HistoryEntry> + 'a {
        self.current_group.past.iter().rev().map(move |entry| {
            HistoryEntry {
                pipeline_id: Some(self.pipeline_id),
                frame_id: self.id,
                instant: entry.instant,
                url: entry.url.clone()
            }
        }).chain(self.past.iter().rev().flat_map(|group| {
            group.entries.past.iter().chain(once(&group.entries.current))
                .chain(group.entries.future.iter()).map(move |entry| {
                    HistoryEntry {
                        pipeline_id: group.pipeline_id,
                        frame_id: group.frame_id,
                        instant: entry.instant,
                        url: entry.url.clone()
                    }
            })
        }))
    }
}

/// A representation of a session history entry. This just aggregates all the data needed
/// when the constellation is handling traversal, but it does not represent how a history
/// entry is actually stored.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The pipeline of this entry.
    pub pipeline_id: Option<PipelineId>,

    /// The frame that owns this entry.
    pub frame_id: FrameId,

    /// The timestamp when this entry was added to the session history.
    pub instant: Instant,

    /// The url of this entry.
    pub url: ServoUrl,
}

/// A group of consecutive entries in the session history that share a pipeline id.
#[derive(Debug)]
pub struct FrameStateGroup {
    /// The pipeline for these entries in the session history,
    /// None if the pipeline has been discarded
    pub pipeline_id: Option<PipelineId>,

    /// The frame that this session history entry group is part of
    pub frame_id: FrameId,

    /// The history entries that share this pipeline.
    entries: FrameStateEntries,
}

/// A group of entries that share a pipeline id and are traversable. This is only used for the
/// active group on a `Frame` as an inactive group has no concept of a currently active entry or
/// sets of future and past entries.
#[derive(Debug)]
struct FrameStateEntries {
    /// The index of the currently active entry
    current: FrameState,

    /// The past entries.
    past: Vec<FrameState>,

    /// The future entries.
    future: Vec<FrameState>,
}

impl FrameStateEntries {
    /// Traverses to the given entry.
    fn traverse_to_entry(&mut self, entry: &HistoryEntry) -> bool {
        use std::cmp::Ordering;

        match entry.instant.cmp(&self.current.instant) {
            Ordering::Equal => return true,
            Ordering::Less => {
                // Traversing the past
                while let Some(prev) = self.past.pop() {
                    let old = replace(&mut self.current, prev);
                    self.future.push(old);
                    if self.current.instant <= entry.instant {
                        return true;
                    }
                }
            },
            Ordering::Greater => {
                // Traversing the future
                while let Some(next) = self.future.pop() {
                    let old = replace(&mut self.current, next);
                    self.past.push(old);
                    if self.current.instant >= entry.instant {
                        return true;
                    }
                }
            },
        }
        false
    }
}

impl FrameStateEntries {
    fn new(url: ServoUrl) -> FrameStateEntries {
        FrameStateEntries {
            current: FrameState {
                instant: Instant::now(),
                url: url,
            },
            past: vec!(),
            future: vec!(),
        }
    }
}

/// An entry in a frame's session history.
///
/// When we operate on the joint session history, entries are sorted chronologically,
/// so we timestamp the entries by when the entry was added to the session history.
#[derive(Debug, Clone)]
pub struct FrameState {
    /// The timestamp for when the session history entry was created
    pub instant: Instant,

    /// The URL for this entry, used to reload the pipeline if it has been discarded
    pub url: ServoUrl,
}

/// Represents a pending change in the frame tree, that will be applied
/// once the new pipeline has loaded and completed initial layout / paint.
pub struct FrameChange {
    /// The frame to change.
    pub frame_id: FrameId,

    /// The pipeline that was currently active at the time the change started.
    /// TODO: can this field be removed?
    pub old_pipeline_id: Option<PipelineId>,

    /// The pipeline for the document being loaded.
    pub new_pipeline_id: PipelineId,

    /// The URL for the document being loaded.
    pub url: ServoUrl,

    /// Is the new document replacing the current document (e.g. a reload)
    /// or pushing it into the session history (e.g. a navigation)?
    pub replace: Option<HistoryEntry>,
}

/// An iterator over a frame tree, returning the fully active frames in
/// depth-first order. Note that this iterator only returns the fully
/// active frames, that is ones where every ancestor frame is
/// in the currently active pipeline of its parent frame.
pub struct FrameTreeIterator<'a> {
    /// The frames still to iterate over.
    pub stack: Vec<FrameId>,

    /// The set of all frames.
    pub frames: &'a HashMap<FrameId, Frame>,

    /// The set of all pipelines.  We use this to find the active
    /// children of a frame, which are the iframes in the currently
    /// active document.
    pub pipelines: &'a HashMap<PipelineId, Pipeline>,
}

impl<'a> Iterator for FrameTreeIterator<'a> {
    type Item = &'a Frame;
    fn next(&mut self) -> Option<&'a Frame> {
        loop {
            let frame_id = match self.stack.pop() {
                Some(frame_id) => frame_id,
                None => return None,
            };
            let frame = match self.frames.get(&frame_id) {
                Some(frame) => frame,
                None => {
                    warn!("Frame {:?} iterated after closure.", frame_id);
                    continue;
                },
            };
            let pipeline = match self.pipelines.get(&frame.pipeline_id) {
                Some(pipeline) => pipeline,
                None => {
                    warn!("Pipeline {:?} iterated after closure.", frame.pipeline_id);
                    continue;
                },
            };
            self.stack.extend(pipeline.children.iter());
            return Some(frame)
        }
    }
}

/// An iterator over a frame tree, returning all frames in depth-first
/// order. Note that this iterator returns all frames, not just the
/// fully active ones.
pub struct FullFrameTreeIterator<'a> {
    /// The frames still to iterate over.
    pub stack: Vec<FrameId>,

    /// The set of all frames.
    pub frames: &'a HashMap<FrameId, Frame>,

    /// The set of all pipelines.  We use this to find the
    /// children of a frame, which are the iframes in all documents
    /// in the session history.
    pub pipelines: &'a HashMap<PipelineId, Pipeline>,
}

impl<'a> Iterator for FullFrameTreeIterator<'a> {
    type Item = &'a Frame;
    fn next(&mut self) -> Option<&'a Frame> {
        let pipelines = self.pipelines;
        loop {
            let frame_id = match self.stack.pop() {
                Some(frame_id) => frame_id,
                None => return None,
            };
            let frame = match self.frames.get(&frame_id) {
                Some(frame) => frame,
                None => {
                    warn!("Frame {:?} iterated after closure.", frame_id);
                    continue;
                },
            };
            let child_frame_ids = frame.past.iter().chain(frame.future.iter())
                .filter_map(|entry| entry.pipeline_id)
                .chain(once(frame.pipeline_id))
                .filter_map(|pipeline_id| pipelines.get(&pipeline_id))
                .flat_map(|pipeline| pipeline.children.iter());
            self.stack.extend(child_frame_ids);
            return Some(frame)
        }
    }
}
