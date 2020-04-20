//! The module contains the implementation of the `SETTINGS` frame and associated flags.

use crate::codec::write_buffer::WriteBuffer;
use crate::solicit::frame::flags::*;
use crate::solicit::frame::Frame;
use crate::solicit::frame::FrameBuilder;
use crate::solicit::frame::FrameHeader;
use crate::solicit::frame::FrameIR;
use crate::solicit::frame::ParseFrameError;
use crate::solicit::frame::ParseFrameResult;
use crate::solicit::frame::RawFrame;
use crate::solicit::stream_id::StreamId;
use crate::solicit::window_size::MAX_WINDOW_SIZE;

pub const SETTINGS_FRAME_TYPE: u8 = 0x4;

/// An enum that lists all valid settings that can be sent in a SETTINGS
/// frame.
///
/// Each setting has a value that is a 32 bit unsigned integer (6.5.1.).
#[derive(Clone, PartialEq, Debug, Copy)]
pub enum HttpSetting {
    HeaderTableSize(u32),
    EnablePush(bool),
    MaxConcurrentStreams(u32),
    InitialWindowSize(u32),
    MaxFrameSize(u32),
    MaxHeaderListSize(u32),
}

impl HttpSetting {
    /// Creates a new `HttpSetting` with the correct variant corresponding to
    /// the given setting id, based on the settings IDs defined in section
    /// 6.5.2.
    pub fn from_id(id: u16, val: u32) -> ParseFrameResult<Option<HttpSetting>> {
        Ok(Some(match id {
            1 => HttpSetting::HeaderTableSize(val),
            2 => {
                let b = match val {
                    0 => false,
                    1 => true,
                    _ => return Err(ParseFrameError::IncorrectSettingsPushValue(val)),
                };
                HttpSetting::EnablePush(b)
            }
            3 => HttpSetting::MaxConcurrentStreams(val),
            4 => {
                if val > MAX_WINDOW_SIZE {
                    // 6.5.2.  Defined SETTINGS Parameters
                    // Values above the maximum flow-control window size of 2^31-1 MUST
                    // be treated as a connection error (Section 5.4.1) of type
                    // FLOW_CONTROL_ERROR.
                    // We handle it in `process_settings_req`
                }
                HttpSetting::InitialWindowSize(val)
            }
            5 => {
                if val < 0x4000 || val >= 0x100_0000 {
                    // 6.5.2.  Defined SETTINGS Parameters
                    // The initial value is 2^14 (16,384) octets.  The value advertised
                    // by an endpoint MUST be between this initial value and the maximum
                    // allowed frame size (2^24-1 or 16,777,215 octets), inclusive.
                    // Values outside this range MUST be treated as a connection error
                    // (Section 5.4.1) of type PROTOCOL_ERROR.
                    return Err(ParseFrameError::IncorrectSettingsMaxFrameSize(val));
                }
                HttpSetting::MaxFrameSize(val)
            }
            6 => HttpSetting::MaxHeaderListSize(val),
            _ => return Ok(None),
        }))
    }

    /// Creates a new `HttpSetting` by parsing the given buffer of 6 bytes,
    /// which contains the raw byte representation of the setting, according
    /// to the "SETTINGS format" defined in section 6.5.1.
    ///
    /// The `raw_setting` parameter should have length at least 6 bytes, since
    /// the length of the raw setting is exactly 6 bytes.
    ///
    /// # Panics
    ///
    /// If given a buffer shorter than 6 bytes, the function will panic.
    fn parse_setting(raw_setting: &[u8]) -> ParseFrameResult<Option<HttpSetting>> {
        let id: u16 = ((raw_setting[0] as u16) << 8) | (raw_setting[1] as u16);
        let val: u32 = unpack_octets_4!(raw_setting, 2, u32);

        HttpSetting::from_id(id, val)
    }

    /// Returns the setting ID as an unsigned 16 bit integer, as defined in
    /// section 6.5.2.
    pub fn get_id(&self) -> u16 {
        match *self {
            HttpSetting::HeaderTableSize(_) => 1,
            HttpSetting::EnablePush(_) => 2,
            HttpSetting::MaxConcurrentStreams(_) => 3,
            HttpSetting::InitialWindowSize(_) => 4,
            HttpSetting::MaxFrameSize(_) => 5,
            HttpSetting::MaxHeaderListSize(_) => 6,
        }
    }

    /// Gets the setting value by unpacking it from the wrapped `u32`.
    pub fn get_val(&self) -> u32 {
        match *self {
            HttpSetting::HeaderTableSize(val)
            | HttpSetting::MaxConcurrentStreams(val)
            | HttpSetting::InitialWindowSize(val)
            | HttpSetting::MaxFrameSize(val)
            | HttpSetting::MaxHeaderListSize(val) => val,
            HttpSetting::EnablePush(true) => 1,
            HttpSetting::EnablePush(false) => 0,
        }
    }

    /// Serializes a setting into its "on-the-wire" representation of 6 octets,
    /// according to section 6.5.1.
    fn serialize(&self) -> [u8; 6] {
        let (id, val) = (self.get_id(), self.get_val());
        [
            ((id >> 8) & 0x00FF) as u8,
            ((id) & 0x00FF) as u8,
            (((val >> 24) & 0x000000FF) as u8),
            (((val >> 16) & 0x000000FF) as u8),
            (((val >> 8) & 0x000000FF) as u8),
            (((val) & 0x000000FF) as u8),
        ]
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct HttpSettings {
    pub header_table_size: u32,
    pub enable_push: bool,
    pub max_concurrent_streams: u32,
    pub initial_window_size: u32,
    pub max_frame_size: u32,
    pub max_header_list_size: u32,
}

impl HttpSettings {
    pub fn apply(&mut self, setting: HttpSetting) {
        match setting {
            HttpSetting::HeaderTableSize(s) => self.header_table_size = s,
            HttpSetting::EnablePush(e) => self.enable_push = e,
            HttpSetting::MaxConcurrentStreams(m) => self.max_concurrent_streams = m,
            HttpSetting::InitialWindowSize(s) => self.initial_window_size = s,
            HttpSetting::MaxFrameSize(s) => self.max_frame_size = s,
            HttpSetting::MaxHeaderListSize(s) => self.max_header_list_size = s,
        }
    }

    pub fn apply_from_frame(&mut self, frame: &SettingsFrame) {
        for s in &frame.settings {
            self.apply(*s);
        }
    }
}

/// An enum representing the flags that a `SettingsFrame` can have.
/// The integer representation associated to each variant is that flag's
/// bitmask.
///
/// HTTP/2 spec, section 6.5.
#[derive(Clone, PartialEq, Debug, Copy)]
pub enum SettingsFlag {
    Ack = 0x1,
}

impl Flag for SettingsFlag {
    #[inline]
    fn bitmask(&self) -> u8 {
        *self as u8
    }

    fn flags() -> &'static [Self] {
        static FLAGS: &'static [SettingsFlag] = &[SettingsFlag::Ack];
        FLAGS
    }
}

/// A struct representing the SETTINGS frames of HTTP/2, as defined in the
/// HTTP/2 spec, section 6.5.
///
/// The struct does not try to prevent the client from creating malformed
/// SETTINGS frames, such as ones that have the ACK flag set along with some
/// settings values. The users are responsible to follow the prescribed rules
/// before sending the frame to the peer.
///
/// On parsing received frames, it treats the following as errors:
///
/// - ACK flag and a number of settings both set
/// - Payload length not a multiple of 6
/// - Stream ID not zero (SETTINGS frames MUST be associated to stream 0)
///
/// What is *not* treated as an error (for now) are settings values out of
/// allowed bounds such as a EnablePush being set to something other than 0 or
/// 1.
#[derive(PartialEq, Debug, Clone)]
pub struct SettingsFrame {
    /// Contains all the settings that are currently set in the frame. It is
    /// safe to access this field (to read, add, or remove settings), even
    /// though a helper method `add_setting` exists.
    pub settings: Vec<HttpSetting>,
    /// Represents the flags currently set on the `SettingsFrame`, packed into
    /// a single byte.
    flags: Flags<SettingsFlag>,
}

impl SettingsFrame {
    /// Creates a new empty `SettingsFrame`
    pub fn new() -> SettingsFrame {
        SettingsFrame {
            settings: Vec::new(),
            // By default, no flags are set
            flags: Flags::default(),
        }
    }

    /// A convenience constructor that returns a `SettingsFrame` with the ACK
    /// flag already set and no settings.
    pub fn new_ack() -> SettingsFrame {
        SettingsFrame {
            settings: Vec::new(),
            flags: SettingsFlag::Ack.to_flags(),
        }
    }

    // Create a SETTINGS frame with given list of settings
    pub fn from_settings(settings: Vec<HttpSetting>) -> SettingsFrame {
        SettingsFrame {
            settings: settings,
            flags: Flags::default(),
        }
    }

    /// Adds the given setting to the frame.
    pub fn add_setting(&mut self, setting: HttpSetting) {
        self.settings.push(setting);
    }

    /// Sets the ACK flag for the frame. This method is just a convenience
    /// method for calling `frame.set_flag(SettingsFlag::Ack)`.
    pub fn set_ack(&mut self) {
        self.set_flag(SettingsFlag::Ack)
    }

    /// Checks whether the `SettingsFrame` has an ACK attached to it.
    pub fn is_ack(&self) -> bool {
        self.flags.is_set(SettingsFlag::Ack)
    }

    /// Returns the total length of the payload in bytes.
    fn payload_len(&self) -> u32 {
        // Each setting is represented with 6 bytes =>
        6 * self.settings.len() as u32
    }

    /// Parses the given buffer, considering it a representation of a settings
    /// frame payload.
    ///
    /// # Returns
    ///
    /// A `Vec` of settings that are set by the given payload.
    ///
    /// Any unknown setting is ignored, as per the HTTP/2 spec requirement.
    ///
    /// If the frame is invalid (i.e. the length of the payload is not a
    /// multiple of 6) it returns `None`.
    fn parse_payload(payload: &[u8]) -> ParseFrameResult<Vec<HttpSetting>> {
        if payload.len() % 6 != 0 {
            return Err(ParseFrameError::ProtocolError);
        }

        // Iterates through chunks of the raw payload of size 6 bytes and
        // parses each of them into an `HttpSetting`
        let mut settings = Vec::new();
        for chunk in payload.chunks(6) {
            if let Some(setting) = HttpSetting::parse_setting(chunk)? {
                settings.push(setting);
            }
        }
        Ok(settings)
    }

    /// Sets the given flag for the frame.
    pub fn set_flag(&mut self, flag: SettingsFlag) {
        self.flags.set(flag);
    }
}

impl Frame for SettingsFrame {
    /// The type that represents the flags that the particular `Frame` can take.
    /// This makes sure that only valid `Flag`s are used with each `Frame`.
    type FlagType = SettingsFlag;

    /// Creates a new `SettingsFrame` with the given `RawFrame` (i.e. header and
    /// payload), if possible.
    ///
    /// # Returns
    ///
    /// `None` if a valid `SettingsFrame` cannot be constructed from the given
    /// `RawFrame`. The stream ID *must* be 0 in order for the frame to be
    /// valid. If the `ACK` flag is set, there MUST not be a payload. The
    /// total payload length must be multiple of 6.
    ///
    /// Otherwise, returns a newly constructed `SettingsFrame`.
    fn from_raw(raw_frame: &RawFrame) -> ParseFrameResult<SettingsFrame> {
        // Unpack the header
        let FrameHeader {
            payload_len,
            frame_type,
            flags,
            stream_id,
        } = raw_frame.header();
        // Check that the frame type is correct for this frame implementation
        if frame_type != SETTINGS_FRAME_TYPE {
            return Err(ParseFrameError::InternalError);
        }
        // Check that the length given in the header matches the payload
        // length; if not, something went wrong and we do not consider this a
        // valid frame.
        if (payload_len as usize) != raw_frame.payload().len() {
            return Err(ParseFrameError::InternalError);
        }
        // Check that the SETTINGS frame is associated to stream 0
        if stream_id != 0 {
            return Err(ParseFrameError::StreamIdMustBeNonZero);
        }
        if (flags & SettingsFlag::Ack.bitmask()) != 0 {
            return if payload_len == 0 {
                // Ack is set and there's no payload => just an Ack frame
                Ok(SettingsFrame {
                    settings: Vec::new(),
                    flags: Flags::new(flags),
                })
            } else {
                // The SETTINGS flag MUST not have a payload if Ack is set
                Err(ParseFrameError::ProtocolError)
            };
        }

        let settings = SettingsFrame::parse_payload(&raw_frame.payload())?;
        Ok(SettingsFrame {
            settings,
            flags: Flags::new(flags),
        })
    }

    /// Tests if the given flag is set for the frame.
    fn flags(&self) -> Flags<SettingsFlag> {
        self.flags
    }

    /// Returns the `StreamId` of the stream to which the frame is associated.
    ///
    /// A `SettingsFrame` always has to be associated to stream `0`.
    fn get_stream_id(&self) -> StreamId {
        0
    }

    /// Returns a `FrameHeader` based on the current state of the `Frame`.
    fn get_header(&self) -> FrameHeader {
        FrameHeader {
            payload_len: self.payload_len(),
            frame_type: SETTINGS_FRAME_TYPE,
            flags: self.flags.0,
            stream_id: 0,
        }
    }
}

impl FrameIR for SettingsFrame {
    fn serialize_into(self, b: &mut WriteBuffer) {
        b.write_header(self.get_header());
        for setting in &self.settings {
            b.extend_from_slice(&setting.serialize());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HttpSetting, SettingsFrame};
    use crate::solicit::frame::FrameIR;
    use crate::solicit::frame::{pack_header, Frame, FrameHeader};
    use crate::solicit::tests::common::raw_frame_from_parts;

    /// Tests that a `SettingsFrame` correctly handles a SETTINGS frame with
    /// no ACK flag and only a single setting.
    #[test]
    fn test_settings_frame_parse_no_ack_one_setting() {
        let payload = [0, 1, 0, 0, 0, 1];
        // A header with the flag indicating no padding
        let header = FrameHeader::new(payload.len() as u32, 4, 0, 0);

        let raw = raw_frame_from_parts(header.clone(), payload.to_vec());
        let frame: SettingsFrame = Frame::from_raw(&raw).unwrap();

        // The frame correctly interprets the settings?
        assert_eq!(frame.settings, vec![HttpSetting::HeaderTableSize(1)]);
        // ...and the headers?
        assert_eq!(frame.get_header(), header);
    }

    /// Tests that a `SettingsFrame` correctly handles a SETTINGS frame with
    /// no ACK flag and multiple settings within the frame.
    #[test]
    fn test_settings_frame_parse_no_ack_multiple_settings() {
        let settings = vec![
            HttpSetting::HeaderTableSize(1),
            HttpSetting::MaxHeaderListSize(5),
            HttpSetting::EnablePush(false),
        ];
        let payload = {
            let mut res: Vec<u8> = Vec::new();
            for s in settings.iter().map(|s| s.serialize()) {
                res.extend(s.to_vec().into_iter());
            }

            res
        };
        let header = FrameHeader::new(payload.len() as u32, 4, 0, 0);

        let raw = raw_frame_from_parts(header.clone(), payload.to_vec());
        let frame: SettingsFrame = Frame::from_raw(&raw).unwrap();

        // The frame correctly interprets the settings?
        assert_eq!(frame.settings, settings);
        // ...and the headers?
        assert_eq!(frame.get_header(), header);
        assert!(!frame.is_ack());
    }

    /// Tests that a `SettingsFrame` correctly handles a SETTINGS frame with
    /// no ACK and multiple *duplicate* settings within the frame.
    #[test]
    fn test_settings_frame_parse_no_ack_duplicate_settings() {
        let settings = vec![
            HttpSetting::HeaderTableSize(1),
            HttpSetting::MaxHeaderListSize(5),
            HttpSetting::EnablePush(false),
            HttpSetting::HeaderTableSize(2),
        ];
        let payload = {
            let mut res: Vec<u8> = Vec::new();
            for s in settings.iter().map(|s| s.serialize()) {
                res.extend(s.to_vec().into_iter());
            }

            res
        };
        let header = FrameHeader::new(payload.len() as u32, 4, 0, 0);

        let raw = raw_frame_from_parts(header.clone(), payload.to_vec());
        let frame: SettingsFrame = Frame::from_raw(&raw).unwrap();

        // All the settings are returned, even the duplicates
        assert_eq!(frame.settings, settings);
        // ...and the headers?
        assert_eq!(frame.get_header(), header);
        assert!(!frame.is_ack());
    }

    /// Tests that a `SettingsFrame` correctly handles a SETTING frame with no
    /// ACK and an unknown setting within the frame. The unknown setting is
    /// simply ignored.
    #[test]
    fn test_settings_frame_parse_no_ack_unknown_setting() {
        let settings = vec![
            HttpSetting::HeaderTableSize(1),
            HttpSetting::MaxHeaderListSize(5),
        ];
        let payload = {
            let mut res: Vec<u8> = Vec::new();
            for s in settings.iter().map(|s| s.serialize()) {
                res.extend(s.to_vec().into_iter());
            }
            res.extend(vec![0, 10, 0, 0, 0, 0].into_iter());
            for s in settings.iter().map(|s| s.serialize()) {
                res.extend(s.to_vec().into_iter());
            }

            res
        };
        let header = FrameHeader::new(payload.len() as u32, 4, 0, 0);

        let raw = raw_frame_from_parts(header.clone(), payload.to_vec());
        let frame: SettingsFrame = Frame::from_raw(&raw).unwrap();

        // All the settings are returned twice, but the unkown isn't found in
        // the returned Vec. For now, we ignore the unknown setting fully, not
        // exposing it in any way to any other higher-level clients.
        assert_eq!(frame.settings.len(), 4);
        assert_eq!(&frame.settings[0..2], &settings[..]);
        assert_eq!(&frame.settings[2..], &settings[..]);
        assert!(!frame.is_ack());
    }

    /// Tests that a `SettingsFrame` correctly handles a SETTINGS frame with an
    /// ACK flag and no settings.
    #[test]
    fn test_settings_frame_parse_ack_no_settings() {
        let payload = [];
        let header = FrameHeader {
            payload_len: payload.len() as u32,
            frame_type: 4,
            flags: 1,
            stream_id: 0,
        };

        let raw = raw_frame_from_parts(header.clone(), payload.to_vec());
        let frame: SettingsFrame = Frame::from_raw(&raw).unwrap();

        // No settings there?
        assert_eq!(frame.settings, vec![]);
        // ...and the headers?
        assert_eq!(frame.get_header(), header);
        // ...and the frame indicates it's an ACK
        assert!(frame.is_ack());
    }

    /// Tests that a `SettingsFrame` correctly handles a SETTINGS frame with an
    /// ACK flag, along with settings. In this case, the frame needs to be
    /// considered invalid.
    #[test]
    fn test_settings_frame_parse_ack_with_settings() {
        let settings = [HttpSetting::EnablePush(false)];
        let payload = {
            let mut res: Vec<u8> = Vec::new();
            for s in settings.iter().map(|s| s.serialize()) {
                res.extend(s.to_vec().into_iter());
            }

            res
        };
        let header = FrameHeader::new(payload.len() as u32, 4, 1, 0);

        let raw = raw_frame_from_parts(header, payload);
        let frame = SettingsFrame::from_raw(&raw);

        assert!(frame.is_err());
    }

    /// Tests that a `SettingsFrame` correctly handles a SETTINGS frame which
    /// was not associated to stream 0 by returning an error.
    #[test]
    fn test_settings_frame_parse_not_stream_zero() {
        let payload = vec![];
        // Header indicates that it is associated to stream 1
        let header = FrameHeader::new(payload.len() as u32, 4, 1, 1);

        let raw = raw_frame_from_parts(header, payload);
        let frame = SettingsFrame::from_raw(&raw);

        assert!(frame.is_err());
    }

    /// Tests that a `SettingsFrame` correctly handles a SETTINGS frame which
    /// does not have a payload with a number of bytes that's a multiple of 6.
    #[test]
    fn test_settings_frame_parse_not_multiple_of_six() {
        let payload = vec![1, 2, 3];

        let header = FrameHeader::new(payload.len() as u32, 4, 0, 0);

        let raw = raw_frame_from_parts(header, payload);
        let frame = SettingsFrame::from_raw(&raw);

        assert!(frame.is_err());
    }

    /// Tests that a `SettingsFrame` gets correctly serialized when it contains
    /// only settings and no ACK.
    #[test]
    fn test_settings_frame_serialize_no_ack_settings() {
        let mut frame = SettingsFrame::new();
        frame.add_setting(HttpSetting::EnablePush(false));
        let expected = {
            let mut res: Vec<u8> = Vec::new();
            res.extend(
                pack_header(&FrameHeader {
                    payload_len: 6,
                    frame_type: 4,
                    flags: 0,
                    stream_id: 0,
                })
                .to_vec()
                .into_iter(),
            );
            res.extend(
                HttpSetting::EnablePush(false)
                    .serialize()
                    .to_vec()
                    .into_iter(),
            );

            res
        };

        let serialized = frame.serialize_into_vec();

        assert_eq!(serialized, expected);
    }

    /// Tests that a `SettingsFrame` gets correctly serialized when it contains
    /// multiple settings and no ACK.
    #[test]
    fn test_settings_frame_serialize_no_ack_multiple_settings() {
        let mut frame = SettingsFrame::new();
        frame.add_setting(HttpSetting::EnablePush(false));
        frame.add_setting(HttpSetting::MaxHeaderListSize(0));
        let expected = {
            let mut res: Vec<u8> = Vec::new();
            res.extend(
                pack_header(&FrameHeader {
                    payload_len: 6 * 2,
                    frame_type: 4,
                    flags: 0,
                    stream_id: 0,
                })
                .to_vec()
                .into_iter(),
            );
            res.extend(
                HttpSetting::EnablePush(false)
                    .serialize()
                    .to_vec()
                    .into_iter(),
            );
            res.extend(
                HttpSetting::MaxHeaderListSize(0)
                    .serialize()
                    .to_vec()
                    .into_iter(),
            );

            res
        };

        let serialized = frame.serialize_into_vec();

        assert_eq!(serialized, expected);
    }

    /// Tests that a `SettingsFrame` gets correctly serialized when it contains
    /// multiple settings and no ACK.
    #[test]
    fn test_settings_frame_serialize_ack() {
        let frame = SettingsFrame::new_ack();
        let expected = pack_header(&FrameHeader {
            payload_len: 0,
            frame_type: 4,
            flags: 1,
            stream_id: 0,
        })
        .to_vec();

        let serialized = frame.serialize_into_vec();

        assert_eq!(serialized, expected);
    }

    /// Tests that the `HttpSetting::parse_setting` method correctly creates
    /// settings from raw bytes.
    #[test]
    fn test_setting_deserialize() {
        {
            let buf = [0, 1, 0, 0, 1, 0];

            let setting = HttpSetting::parse_setting(&buf).unwrap().unwrap();

            assert_eq!(setting, HttpSetting::HeaderTableSize(1 << 8));
        }
        {
            let buf = [0, 2, 0, 0, 0, 1];

            let setting = HttpSetting::parse_setting(&buf).unwrap().unwrap();

            assert_eq!(setting, HttpSetting::EnablePush(true));
        }
        {
            let buf = [0, 3, 0, 0, 0, 0];

            let setting = HttpSetting::parse_setting(&buf).unwrap().unwrap();

            assert_eq!(setting, HttpSetting::MaxConcurrentStreams(0));
        }
        {
            let buf = [0, 4, 0, 0, 0, 1];

            let setting = HttpSetting::parse_setting(&buf).unwrap().unwrap();

            assert_eq!(setting, HttpSetting::InitialWindowSize(1));
        }
        {
            let buf = [0, 6, 0, 0, 0, 255];

            let setting = HttpSetting::parse_setting(&buf).unwrap().unwrap();

            assert_eq!(setting, HttpSetting::MaxHeaderListSize((1 << 8) - 1));
        }
        {
            let buf = [0, 7, 0, 0, 0, 255];

            let setting = HttpSetting::parse_setting(&buf).unwrap();

            assert!(setting.is_none());
        }
        {
            let buf = [0, 0, 0, 0, 0, 255];

            let setting = HttpSetting::parse_setting(&buf).unwrap();

            assert!(setting.is_none());
        }
    }

    /// Tests that the `HttpSetting::serialize` method correctly creates
    /// a 6 byte buffer based on the given setting.
    #[test]
    fn test_setting_serialize() {
        {
            let buf = [0, 1, 0, 0, 1, 0];

            let setting = HttpSetting::HeaderTableSize(1 << 8);

            assert_eq!(buf, setting.serialize());
        }
        {
            let buf = [0, 2, 0, 0, 0, 1];

            let setting = HttpSetting::EnablePush(true);

            assert_eq!(buf, setting.serialize());
        }
        {
            let buf = [0, 3, 0, 0, 0, 0];

            let setting = HttpSetting::MaxConcurrentStreams(0);

            assert_eq!(buf, setting.serialize());
        }
        {
            let buf = [0, 4, 0, 0, 0, 1];

            let setting = HttpSetting::InitialWindowSize(1);

            assert_eq!(buf, setting.serialize());
        }
        {
            let buf = [0, 5, 0, 0, 0, 255];

            let setting = HttpSetting::MaxFrameSize((1 << 8) - 1);

            assert_eq!(buf, setting.serialize());
        }
        {
            let buf = [0, 6, 0, 0, 0, 255];

            let setting = HttpSetting::MaxHeaderListSize((1 << 8) - 1);

            assert_eq!(buf, setting.serialize());
        }
    }
}
