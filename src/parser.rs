use std::str::from_utf8;
use nom::{be_u8, be_u16, be_i16, be_u32, be_u64, anychar, IResult};

// HERE'S OUR CORE DATA TYPES / STRUCTS / ENUMS, YAYYYYY

#[derive(Debug,PartialEq,Eq)]
pub struct Lead<'a> {
    pub major:          u8,         // file format major version number (0x03)
    pub minor:          u8,         // file format minor version number (0x00)
    pub rpm_type:       i16,        // package type (0x00 = binary, 0x01 = source)
    pub archnum:        i16,        // if binary: package arch (0x01 = i386, etc.)
    pub name:           &'a str,    // actually a NUL-terminated [u8;66]
    pub osnum:          i16,        // if binary: package OS (0x01 = Linux)
    pub signature_type: i16,        // package signature type (0x05)
}
// TODO: implement Display

#[derive(Debug,PartialEq,Eq)]
pub enum TagType {
    Null,
    Char,
    Int8,
    Int16,
    Int32,
    Int64,
    String,
    Binary,
}

// convert a u32 to the equivalent TagType variant.
// TODO: surely there's a better way to do this (without FromPrimitive)?
fn u32_to_tagtype(val: u32) -> Result<TagType, &'static str> {
    match val {
        0 => Ok(TagType::Null),
        1 => Ok(TagType::Char),
        2 => Ok(TagType::Int8),
        3 => Ok(TagType::Int16),
        4 => Ok(TagType::Int32),
        5 => Ok(TagType::Int64),
        6|8|9 => Ok(TagType::String),
        7|10|11 => Ok(TagType::Binary),
        _ => Err("Unknown tag type"),
    }
}

#[derive(Debug,PartialEq,Eq)]
pub enum TagValue<'a> {
    Null,
    Char(Vec<char>),
    Int8(Vec<u8>),
    Int16(Vec<u16>),
    Int32(Vec<u32>),
    Int64(Vec<u64>),
    String(Vec<&'a str>),
    Binary(&'a [u8]),
}

#[derive(Debug,PartialEq,Eq)]
pub struct HeaderSectionHeader {
    version: u8,
    count: u32,
    size: u32,
}

#[derive(Debug,PartialEq,Eq)]
pub struct TagEntry {
    tag: u32, // TODO: a Tag enum
    tagtype: TagType,
    offset: u32,
    count: u32,
}

// TODO: implement an iterator for (tag, val) pairs?
#[derive(Debug,PartialEq,Eq)]
pub struct HeaderSection<'a> {
    hdr: HeaderSectionHeader,
    tags: Vec<TagEntry>,
    store: &'a [u8], // TODO: i dunno about this lifetime...
}

// HERE'S THE PARSER STUFF YAYYYYY

// quick parser function to grab a NUL-terminated string
named!(cstr(&[u8]) -> &str, map_res!(take_until!("\0"), from_utf8));

// macro that gets a fixed-size NUL-terminated string, tossing the NUL bytes
macro_rules! take_cstr (
    ($i:expr, $maxlen:expr) => (
        chain!($i,
            s: cstr ~
            length: expr_opt!( { ($maxlen as usize).checked_sub(s.len()) } ) ~
            take!(length),
            || {s}
        )
    );
);

named!(parse_lead(&[u8]) -> Lead,
    chain!(
        tag!([0xED, 0xAB, 0xEE, 0xDB]) ~ // the tilde chains items together
        maj:  be_u8  ~
        min:  be_u8  ~
        typ:  be_i16 ~
        arch: be_i16 ~
        name: take_cstr!(66) ~
        os:   be_i16 ~
        sig:  be_i16 ~
        take!(16), // the chain ends with a comma
        // closure yields our return value
        || { Lead {major: maj, minor: min, rpm_type: typ, archnum: arch,
                   name: name, osnum: os, signature_type: sig} }
  )
);

named!(parse_section_header(&[u8]) -> HeaderSectionHeader,
    chain!(
        tag!([0x8E, 0xAD, 0xE8]) ~
        v: be_u8  ~
        take!(4)  ~
        c: be_u32 ~
        s: be_u32,
        || { HeaderSectionHeader {version:v, count:c, size:s} }
    )
);

named!(parse_tag_entry(&[u8]) -> TagEntry,
    chain!(
        tag: be_u32 ~ // TODO: enum for Tags?
        typ: map_res!(be_u32, u32_to_tagtype) ~
        off: be_u32 ~
        cnt: be_u32,
        || { TagEntry {tag:tag, tagtype:typ, offset:off, count:cnt} }
    )
);

fn _parse_tagval<'a, 'b>(store: &'a [u8], tag: &'a TagEntry) -> IResult<&'a [u8], TagValue<'a>> {
    let count = tag.count as usize;
    match tag.tagtype {
        TagType::Null   => value!(store, TagValue::Null),
        TagType::Char   => count!(store, anychar, count).map(|v| TagValue::Char(v)),
        TagType::Int8   => count!(store, be_u8, count).map(  |v| TagValue::Int8(v)),
        TagType::Int16  => count!(store, be_u16, count).map( |v| TagValue::Int16(v)),
        TagType::Int32  => count!(store, be_u32, count).map( |v| TagValue::Int32(v)),
        TagType::Int64  => count!(store, be_u64, count).map( |v| TagValue::Int64(v)),
        TagType::String => count!(store, cstr, count).map(   |v| TagValue::String(v)),
        TagType::Binary =>  take!(store, count).map(         |v| TagValue::Binary(v)),
    }
}

fn parse_tagval<'a>(i: &'a [u8], tag: &'a TagEntry) -> IResult<&'a [u8], TagValue<'a>> {
    peek!(i,
        chain!(
            take!(tag.offset as usize) ~
            val: apply!(_parse_tagval, tag),
            || { val }
        )
    )
}

named!(parse_section(&[u8]) -> HeaderSection,
    chain!(
        hdr: parse_section_header ~
        tags: count!(parse_tag_entry, hdr.count as usize) ~
        store: take!(hdr.size),
        //vals: call!(tags.iter().map(|t:&TagEntry|t.parse_tagval(store)).collect()),
        || { HeaderSection { hdr: hdr, tags: tags, store: store } }
    )
);

// parse the entire RPM header into (Lead, Signature, Header)
named!(parse_header(&[u8]) -> (Lead, HeaderSection, HeaderSection),
    chain!(
        lead: parse_lead ~
        sig: parse_section ~
        take!(if sig.hdr.size % 8 != 0 {8-(sig.hdr.size%8)} else {0}) ~
        hdr: parse_section,
        || { (lead, sig, hdr) }
    )
);

/*************************************************************
 * BELOW HERE BE TESTS!! WHEEEEE!
 *************************************************************/

#[cfg(test)]
use nom::{Err, ErrorKind, Needed};

#[test]
fn parse_lead_bad_magic() {
    let bytes = &[0; 0x60];
    assert_eq!(parse_lead(bytes),
        IResult::Error(Err::Position(ErrorKind::Tag, &bytes[..]))
    )
}

#[test]
fn parse_lead_empty() {
    let bytes = b"";
    assert_eq!(parse_lead(bytes),
        IResult::Incomplete(Needed::Size(4))
    )
}

#[test]
fn parse_lead_short() {
    let bytes = b"\xED\xAB\xEE\xDB\x03\x00";
    assert_eq!(parse_lead(bytes),
        IResult::Incomplete(Needed::Size(8)) // WW: so.. why is this Size(8)?
    )
}

#[test]
fn parse_lead_ok() {
    let bytes = &include_bytes!("../tests/rpms/binary.x86_64.rpm")[..0x60];
    assert_eq!(parse_lead(bytes), IResult::Done(&b""[..],
        Lead {
            major: 3,
            minor: 0,
            rpm_type: 0,
            archnum: 1,
            name: "hardlink-1:1.0-23.fc24",
            osnum: 1,
            signature_type: 5,
        }
    ))
}

#[test]
fn parse_section_header_ok() {
    let bytes = &include_bytes!("../tests/rpms/binary.x86_64.rpm")[0x60..0x70];
    assert_eq!(parse_section_header(bytes), IResult::Done(&b""[..],
        HeaderSectionHeader { version: 1, count: 8, size: 0x1484 }
    ))
}

#[test]
fn parse_tag_entry_ok() {
    let bytes = &include_bytes!("../tests/rpms/binary.x86_64.rpm")[0x70..0x80];
    assert_eq!(parse_tag_entry(bytes), IResult::Done(&b""[..],
      TagEntry { tag: 0x3e, tagtype: TagType::Binary, offset:0x1474, count:0x10 }
    ))
}

#[test]
fn parse_tag_entry_bad_tagtype() {
    let bytes = b"\0\0\0\xAA\0\0\0\xBB\0\0\0\xCC\0\0\0\xDD";
    assert_eq!(parse_tag_entry(bytes),
        IResult::Error(Err::Position(ErrorKind::MapRes, &bytes[4..]))
    )
}

#[test]
fn parse_tagval_str() {
    let store = &include_bytes!("../tests/rpms/binary.x86_64.rpm")[0x1968..0x313a];
    let tag = TagEntry { tag:0x03e8, tagtype:TagType::String, offset:0x0002, count:1 };
    let (rest, val) = parse_tagval(store, &tag).unwrap();
    assert_eq!(parse_tagval(store, &tag),
               IResult::Done(store, TagValue::String(vec!["hardlink"])))
}

#[test]
fn parse_full_header_ok() {
    let bytes = &include_bytes!("../tests/rpms/binary.x86_64.rpm")[..];
    let (rest, (lead, sig, hdr)) = parse_header(bytes).unwrap();
    assert_eq!(rest[..4], b"\xfd7zX"[..]); // XZ magic for the payload start
    assert_eq!(lead.name, "hardlink-1:1.0-23.fc24");
    assert_eq!(sig.tags, vec![
        TagEntry { tag:0x03e, tagtype:TagType::Binary, offset:0x1474, count:0x10 },
        TagEntry { tag:0x10c, tagtype:TagType::Binary, offset:0x0000, count:0x218 },
        TagEntry { tag:0x10d, tagtype:TagType::String, offset:0x0218, count:0x1 },
        TagEntry { tag:0x3e8, tagtype:TagType::Int32,  offset:0x0244, count:0x1 },
        TagEntry { tag:0x3ea, tagtype:TagType::Binary, offset:0x0248, count:0x218 },
        TagEntry { tag:0x3ec, tagtype:TagType::Binary, offset:0x0460, count:0x10 },
        TagEntry { tag:0x3ef, tagtype:TagType::Int32,  offset:0x0470, count:0x1 },
        TagEntry { tag:0x3f0, tagtype:TagType::Binary, offset:0x0474, count:0x1000 },
    ]);
    assert_eq!(hdr.hdr.count, 0x3e)
}
