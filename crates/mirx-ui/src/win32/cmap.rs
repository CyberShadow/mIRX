//! A reader for the OpenType `cmap` table: which glyph a font draws for a
//! character. GDI's own lookups (`GetGlyphIndicesW`) take UTF-16 units, so
//! they cannot answer for characters above the BMP, such as emoji.
//!
//! Font bytes are outside input: every read is bounds-checked and a malformed
//! table is an error, never a panic. `parse` validates every index a lookup
//! can compute, so `glyph` itself cannot fail.

/// A format-4 segment: `start..=end` maps through `delta`, or through
/// `glyph_ids[base + (c - start)]` (plus `delta`) when `base` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment4 {
    start: u16,
    end: u16,
    delta: u16,
    base: Option<usize>,
}

/// A format-12 group: `start..=end` maps to consecutive glyphs from
/// `start_glyph`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Group12 {
    start: u32,
    end: u32,
    start_glyph: u16,
}

/// The two subtable formats fonts ship: format 4 covers the BMP, format 12
/// all of Unicode (emoji fonts need it).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Subtable {
    Format4 {
        segments: Vec<Segment4>,
        glyph_ids: Vec<u16>,
    },
    Format12 {
        groups: Vec<Group12>,
    },
}

/// A font's Unicode character map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Cmap {
    subtable: Subtable,
}

/// The `(platform, encoding)` records mirx reads, most preferred first, with
/// the subtable format each one is defined to have.
const UNICODE_RECORDS: &[(u16, u16, u16)] = &[
    // Windows, Unicode full repertoire.
    (3, 10, 12),
    // Unicode, Unicode 2.0+ full repertoire.
    (0, 4, 12),
    // Windows, Unicode BMP.
    (3, 1, 4),
    // Unicode, Unicode 2.0+ BMP.
    (0, 3, 4),
];

struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn u16(&self, offset: usize) -> Result<u16, String> {
        self.bytes
            .get(offset..offset + 2)
            .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
            .ok_or_else(|| format!("cmap: read past the end at offset {offset}"))
    }

    fn u32(&self, offset: usize) -> Result<u32, String> {
        self.bytes
            .get(offset..offset + 4)
            .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .ok_or_else(|| format!("cmap: read past the end at offset {offset}"))
    }
}

/// Parses a `cmap` table, choosing the best Unicode subtable.
pub(super) fn parse(table: &[u8]) -> Result<Cmap, String> {
    let reader = Reader { bytes: table };
    let record_count = usize::from(reader.u16(2)?);
    let mut records = Vec::with_capacity(record_count);
    for index in 0..record_count {
        let record = 4 + index * 8;
        records.push((
            reader.u16(record)?,
            reader.u16(record + 2)?,
            reader.u32(record + 4)? as usize,
        ));
    }
    let (offset, format) = UNICODE_RECORDS
        .iter()
        .find_map(|&(platform, encoding, format)| {
            records
                .iter()
                .find(|&&(p, e, _)| (p, e) == (platform, encoding))
                .map(|&(_, _, offset)| (offset, format))
        })
        .ok_or_else(|| "cmap: no Unicode subtable".to_owned())?;
    let actual = reader.u16(offset)?;
    if actual != format {
        return Err(format!(
            "cmap: subtable at offset {offset} has format {actual}, expected {format}"
        ));
    }
    let subtable = match format {
        4 => parse_format4(&reader, offset)?,
        12 => parse_format12(&reader, offset)?,
        _ => unreachable!("UNICODE_RECORDS only names formats 4 and 12"),
    };
    Ok(Cmap { subtable })
}

fn parse_format4(reader: &Reader<'_>, offset: usize) -> Result<Subtable, String> {
    let length = usize::from(reader.u16(offset + 2)?);
    let end_offset = offset + length;
    if end_offset > reader.bytes.len() {
        return Err(format!(
            "cmap: format 4 subtable at offset {offset} ends past the table"
        ));
    }
    let seg_count = usize::from(reader.u16(offset + 6)? / 2);
    let ends = offset + 14;
    let starts = ends + seg_count * 2 + 2;
    let deltas = starts + seg_count * 2;
    let range_offsets = deltas + seg_count * 2;
    let glyph_ids_offset = range_offsets + seg_count * 2;
    if glyph_ids_offset > end_offset {
        return Err(format!(
            "cmap: format 4 subtable at offset {offset} is too short for {seg_count} segments"
        ));
    }
    let glyph_ids = (glyph_ids_offset..end_offset)
        .step_by(2)
        .map(|at| reader.u16(at))
        .collect::<Result<Vec<_>, _>>()?;

    let mut segments = Vec::with_capacity(seg_count);
    for index in 0..seg_count {
        let end = reader.u16(ends + index * 2)?;
        let start = reader.u16(starts + index * 2)?;
        let delta = reader.u16(deltas + index * 2)?;
        let range_offset = usize::from(reader.u16(range_offsets + index * 2)?);
        if start > end {
            return Err(format!(
                "cmap: format 4 segment {index} starts after it ends"
            ));
        }
        if let Some(previous) = segments.last().map(|segment: &Segment4| segment.end)
            && start <= previous
        {
            return Err(format!(
                "cmap: format 4 segment {index} is not after the previous one"
            ));
        }
        // idRangeOffset counts bytes from its own slot; as a glyph_ids index
        // that is half of it, minus the slots left in the idRangeOffset
        // array.
        let base = if range_offset == 0 {
            None
        } else {
            let base = (range_offset / 2)
                .checked_sub(seg_count - index)
                .ok_or_else(|| {
                    format!("cmap: format 4 segment {index} points before glyphIdArray")
                })?;
            if base + usize::from(end - start) >= glyph_ids.len() {
                return Err(format!(
                    "cmap: format 4 segment {index} points past glyphIdArray"
                ));
            }
            Some(base)
        };
        segments.push(Segment4 {
            start,
            end,
            delta,
            base,
        });
    }
    Ok(Subtable::Format4 {
        segments,
        glyph_ids,
    })
}

fn parse_format12(reader: &Reader<'_>, offset: usize) -> Result<Subtable, String> {
    let group_count = reader.u32(offset + 12)? as usize;
    let mut groups: Vec<Group12> = Vec::with_capacity(group_count.min(reader.bytes.len() / 12));
    for index in 0..group_count {
        let group = offset + 16 + index * 12;
        let start = reader.u32(group)?;
        let end = reader.u32(group + 4)?;
        let start_glyph = reader.u32(group + 8)?;
        if start > end {
            return Err(format!(
                "cmap: format 12 group {index} starts after it ends"
            ));
        }
        if groups.last().is_some_and(|previous| start <= previous.end) {
            return Err(format!(
                "cmap: format 12 group {index} is not after the previous one"
            ));
        }
        let start_glyph = u16::try_from(start_glyph)
            .ok()
            .filter(|&start_glyph| u32::from(start_glyph) + (end - start) <= u32::from(u16::MAX))
            .ok_or_else(|| format!("cmap: format 12 group {index} maps past glyph 65535"))?;
        groups.push(Group12 {
            start,
            end,
            start_glyph,
        });
    }
    Ok(Subtable::Format12 { groups })
}

impl Cmap {
    /// The glyph the font draws for `c`; `None` when it has none (glyph 0,
    /// `.notdef`, is no glyph).
    pub(super) fn glyph(&self, c: char) -> Option<u16> {
        let code = u32::from(c);
        let glyph = match &self.subtable {
            Subtable::Format4 {
                segments,
                glyph_ids,
            } => {
                let code = u16::try_from(code).ok()?;
                let index = segments.partition_point(|segment| segment.end < code);
                let segment = segments
                    .get(index)
                    .filter(|segment| segment.start <= code)?;
                match segment.base {
                    None => code.wrapping_add(segment.delta),
                    Some(base) => match glyph_ids[base + usize::from(code - segment.start)] {
                        0 => 0,
                        glyph => glyph.wrapping_add(segment.delta),
                    },
                }
            }
            Subtable::Format12 { groups } => {
                let index = groups.partition_point(|group| group.end < code);
                let group = groups.get(index).filter(|group| group.start <= code)?;
                group.start_glyph + (code - group.start) as u16
            }
        };
        (glyph != 0).then_some(glyph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A format-4 segment for the builder: `(start, end, delta,
    /// glyph ids)`; with glyph ids, the segment maps through glyphIdArray.
    type TestSegment = (u16, u16, i16, Vec<u16>);

    fn format4(segments: &[TestSegment]) -> Vec<u8> {
        let seg_count = segments.len();
        let mut ends = Vec::new();
        let mut starts = Vec::new();
        let mut deltas = Vec::new();
        let mut range_offsets = Vec::new();
        let mut glyph_ids = Vec::<u16>::new();
        for (index, (start, end, delta, ids)) in segments.iter().enumerate() {
            ends.push(*end);
            starts.push(*start);
            deltas.push(*delta as u16);
            if ids.is_empty() {
                range_offsets.push(0);
            } else {
                assert_eq!(ids.len(), usize::from(end - start) + 1);
                // Bytes from this idRangeOffset slot to its glyph ids.
                range_offsets.push(((seg_count - index + glyph_ids.len()) * 2) as u16);
                glyph_ids.extend(ids);
            }
        }
        let mut body = Vec::new();
        for array in [
            &ends,
            &vec![0],
            &starts,
            &deltas,
            &range_offsets,
            &glyph_ids,
        ] {
            for value in array.iter() {
                body.extend(value.to_be_bytes());
            }
        }
        let length = 14 + body.len();
        let mut subtable = Vec::new();
        for value in [4u16, length as u16, 0, (seg_count * 2) as u16, 0, 0, 0] {
            subtable.extend(value.to_be_bytes());
        }
        // The `&vec![0]` above is the reservedPad between endCode and
        // startCode.
        subtable.extend(body);
        subtable
    }

    fn format12(groups: &[(u32, u32, u32)]) -> Vec<u8> {
        let mut subtable = Vec::new();
        subtable.extend(12u16.to_be_bytes());
        subtable.extend(0u16.to_be_bytes());
        subtable.extend((16 + groups.len() as u32 * 12).to_be_bytes());
        subtable.extend(0u32.to_be_bytes());
        subtable.extend((groups.len() as u32).to_be_bytes());
        for (start, end, glyph) in groups {
            for value in [start, end, glyph] {
                subtable.extend(value.to_be_bytes());
            }
        }
        subtable
    }

    fn table(subtables: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
        let mut table = Vec::new();
        table.extend(0u16.to_be_bytes());
        table.extend((subtables.len() as u16).to_be_bytes());
        let mut offset = 4 + subtables.len() * 8;
        for (platform, encoding, subtable) in subtables {
            table.extend(platform.to_be_bytes());
            table.extend(encoding.to_be_bytes());
            table.extend((offset as u32).to_be_bytes());
            offset += subtable.len();
        }
        for (_, _, subtable) in subtables {
            table.extend(subtable);
        }
        table
    }

    fn sentinel() -> TestSegment {
        (0xFFFF, 0xFFFF, 1, Vec::new())
    }

    #[test]
    fn format4_maps_through_id_delta() {
        let cmap = parse(&table(&[(
            3,
            1,
            format4(&[(0x41, 0x43, -0x40, vec![]), sentinel()]),
        )]))
        .unwrap();
        assert_eq!(cmap.glyph('A'), Some(1));
        assert_eq!(cmap.glyph('C'), Some(3));
        assert_eq!(cmap.glyph('@'), None);
        assert_eq!(cmap.glyph('D'), None);
    }

    #[test]
    fn format4_maps_through_id_range_offset() {
        let cmap = parse(&table(&[(
            3,
            1,
            format4(&[
                (0x20, 0x21, 0, vec![7, 0]),
                (0x2014, 0x2015, 2, vec![10, 20]),
                sentinel(),
            ]),
        )]))
        .unwrap();
        assert_eq!(cmap.glyph(' '), Some(7));
        // A zero in glyphIdArray is missing, whatever idDelta says.
        assert_eq!(cmap.glyph('!'), None);
        assert_eq!(cmap.glyph('—'), Some(12));
        assert_eq!(cmap.glyph('―'), Some(22));
    }

    #[test]
    fn format4_wraps_id_delta_arithmetic() {
        let cmap = parse(&table(&[(
            3,
            1,
            format4(&[(0xFF00, 0xFF01, 0x0110, vec![]), sentinel()]),
        )]))
        .unwrap();
        assert_eq!(cmap.glyph('\u{FF00}'), Some(0x0010));
    }

    #[test]
    fn format4_sentinel_segment_maps_nothing() {
        let cmap = parse(&table(&[(3, 1, format4(&[sentinel()]))])).unwrap();
        // 0xFFFF + 1 wraps to glyph 0.
        assert_eq!(cmap.glyph('\u{FFFF}'), None);
        assert_eq!(cmap.glyph('A'), None);
    }

    #[test]
    fn format4_has_no_supplementary_characters() {
        let cmap = parse(&table(&[(
            3,
            1,
            format4(&[(0, 0xFFFE, 1, vec![]), sentinel()]),
        )]))
        .unwrap();
        assert_eq!(cmap.glyph('A'), Some(0x42));
        assert_eq!(cmap.glyph('🐶'), None);
    }

    #[test]
    fn format12_maps_groups() {
        let cmap = parse(&table(&[(
            3,
            10,
            format12(&[
                (0x20, 0x7E, 1),
                (0x1F400, 0x1F43F, 500),
                (0x1F600, 0x1F600, 900),
            ]),
        )]))
        .unwrap();
        assert_eq!(cmap.glyph(' '), Some(1));
        assert_eq!(cmap.glyph('~'), Some(95));
        assert_eq!(cmap.glyph('🐀'), Some(500));
        assert_eq!(cmap.glyph('🐶'), Some(500 + 0x36));
        assert_eq!(cmap.glyph('😀'), Some(900));
        assert_eq!(cmap.glyph('😁'), None);
        assert_eq!(cmap.glyph('\u{7F}'), None);
    }

    #[test]
    fn format12_glyph_zero_is_missing() {
        let cmap = parse(&table(&[(3, 10, format12(&[(0x41, 0x42, 0)]))])).unwrap();
        assert_eq!(cmap.glyph('A'), None);
        assert_eq!(cmap.glyph('B'), Some(1));
    }

    #[test]
    fn full_repertoire_subtable_wins_over_bmp() {
        let cmap = parse(&table(&[
            (3, 1, format4(&[(0x41, 0x41, 1, vec![]), sentinel()])),
            (3, 10, format12(&[(0x41, 0x41, 7)])),
        ]))
        .unwrap();
        assert_eq!(cmap.glyph('A'), Some(7));
    }

    #[test]
    fn unicode_platform_subtables_are_read() {
        let cmap = parse(&table(&[(
            0,
            3,
            format4(&[(0x41, 0x41, 1, vec![]), sentinel()]),
        )]))
        .unwrap();
        assert_eq!(cmap.glyph('A'), Some(0x42));
    }

    #[test]
    fn malformed_tables_are_errors() {
        assert_eq!(
            parse(&[0, 0, 0]),
            Err("cmap: read past the end at offset 2".to_owned())
        );
        assert_eq!(
            parse(&table(&[(3, 0, format4(&[sentinel()]))])),
            Err("cmap: no Unicode subtable".to_owned())
        );

        let mut past_end = table(&[(3, 1, format4(&[sentinel()]))]);
        past_end[8..12].copy_from_slice(&1000u32.to_be_bytes());
        assert_eq!(
            parse(&past_end),
            Err("cmap: read past the end at offset 1000".to_owned())
        );

        let mut truncated = table(&[(3, 10, format12(&[(0x41, 0x42, 1)]))]);
        truncated.truncate(truncated.len() - 4);
        assert_eq!(
            parse(&truncated),
            Err("cmap: read past the end at offset 36".to_owned())
        );

        assert_eq!(
            parse(&table(&[(3, 10, format4(&[sentinel()]))])),
            Err("cmap: subtable at offset 12 has format 4, expected 12".to_owned())
        );

        let mut bad_range = format4(&[(0x41, 0x42, 0, vec![1, 2]), sentinel()]);
        // Point segment 0's idRangeOffset far past glyphIdArray.
        let range_offsets = 14 + 2 * 2 + 2 + 2 * 2 + 2 * 2;
        bad_range[range_offsets..range_offsets + 2].copy_from_slice(&200u16.to_be_bytes());
        assert_eq!(
            parse(&table(&[(3, 1, bad_range)])),
            Err("cmap: format 4 segment 0 points past glyphIdArray".to_owned())
        );

        assert_eq!(
            parse(&table(&[(3, 10, format12(&[(0x42, 0x41, 1)]))])),
            Err("cmap: format 12 group 0 starts after it ends".to_owned())
        );
        assert_eq!(
            parse(&table(&[(
                3,
                10,
                format12(&[(0x41, 0x42, 1), (0x42, 0x43, 5)])
            )])),
            Err("cmap: format 12 group 1 is not after the previous one".to_owned())
        );
        assert_eq!(
            parse(&table(&[(3, 10, format12(&[(0x41, 0x42, 0xFFFF)]))])),
            Err("cmap: format 12 group 0 maps past glyph 65535".to_owned())
        );
    }
}
