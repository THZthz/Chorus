//! Content fingerprinting (kernel-api.md §2): FNV-1a 64 over a canonical
//! serialization. The canonical form feeds every `Serialize` value through a
//! hashing serializer that writes tagged little-endian bytes; `BTreeMap`
//! iteration order and fixed struct field order make the byte stream
//! deterministic.

use serde::ser::{
    self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};
use std::fmt;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

const LEN_UNKNOWN: u64 = u64::MAX;

/// The error type of the canonical serializer; it cannot actually occur —
/// the serializer only writes bytes into the hash.
#[derive(Debug)]
pub struct CanonError;

impl fmt::Display for CanonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("canonical serialization cannot fail")
    }
}

impl std::error::Error for CanonError {}

impl ser::Error for CanonError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        CanonError
    }
}

pub struct Canon {
    h: u64,
}

impl Canon {
    fn byte(&mut self, b: u8) {
        self.h ^= b as u64;
        self.h = self.h.wrapping_mul(FNV_PRIME);
    }

    fn u64(&mut self, v: u64) {
        for i in 0..8 {
            self.byte(((v >> (i * 8)) & 0xff) as u8);
        }
    }

    fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }

    fn text(&mut self, s: &str) {
        self.u64(s.len() as u64);
        for b in s.as_bytes() {
            self.byte(*b);
        }
    }
}

/// Fingerprint any serializable value deterministically.
pub fn fingerprint<T: Serialize + ?Sized>(value: &T) -> u64 {
    let mut c = Canon { h: FNV_OFFSET };
    // Infallible by construction: the serializer only folds bytes into the
    // hash and reports `CanonError` from no code path.
    let _ = value.serialize(&mut c);
    c.h
}

macro_rules! forward_int {
    ($($m:ident => $t:ty),* $(,)?) => {
        $(fn $m(self, v: $t) -> Result<Self::Ok, Self::Error> {
            self.byte(b'i');
            self.i64(v as i64);
            Ok(())
        })*
    };
}

macro_rules! forward_uint {
    ($($m:ident => $t:ty),* $(,)?) => {
        $(fn $m(self, v: $t) -> Result<Self::Ok, Self::Error> {
            self.byte(b'u');
            self.u64(v as u64);
            Ok(())
        })*
    };
}

impl<'a> Serializer for &'a mut Canon {
    type Ok = ();
    type Error = CanonError;
    type SerializeSeq = SeqWriter<'a>;
    type SerializeTuple = SeqWriter<'a>;
    type SerializeTupleStruct = SeqWriter<'a>;
    type SerializeTupleVariant = SeqWriter<'a>;
    type SerializeMap = MapWriter<'a>;
    type SerializeStruct = StructWriter<'a>;
    type SerializeStructVariant = StructVariantWriter<'a>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.byte(b'b');
        self.byte(if v { 1 } else { 0 });
        Ok(())
    }

    forward_int!(serialize_i8 => i8, serialize_i16 => i16, serialize_i32 => i32, serialize_i64 => i64);
    forward_uint!(serialize_u8 => u8, serialize_u16 => u16, serialize_u32 => u32, serialize_u64 => u64);

    fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
        self.byte(b'I');
        for i in 0..16 {
            self.byte(((v >> (i * 8)) & 0xff) as u8);
        }
        Ok(())
    }

    fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
        self.byte(b'U');
        for i in 0..16 {
            self.byte(((v >> (i * 8)) & 0xff) as u8);
        }
        Ok(())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.byte(b'f');
        self.u64((v as f64).to_bits());
        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.byte(b'f');
        self.u64(v.to_bits());
        Ok(())
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        self.byte(b'c');
        self.u64(v as u32 as u64);
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.byte(b's');
        self.text(v);
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.byte(b'B');
        self.u64(v.len() as u64);
        for b in v {
            self.byte(*b);
        }
        Ok(())
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.byte(b'0');
        Ok(())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        self.byte(b'1');
        value.serialize(self)
    }

    // Unit shapes carry no information; writing nothing keeps the stream
    // identical for wrapper types.
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.byte(b'v');
        self.u64(variant_index as u64);
        Ok(())
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        self.byte(b'v');
        self.u64(variant_index as u64);
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        self.byte(b'[');
        self.u64(len.map_or(LEN_UNKNOWN, |n| n as u64));
        Ok(SeqWriter { c: self })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.byte(b't');
        self.u64(variant_index as u64);
        self.u64(len as u64);
        Ok(SeqWriter { c: self })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        self.byte(b'{');
        self.u64(len.map_or(LEN_UNKNOWN, |n| n as u64));
        Ok(MapWriter { c: self })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        self.byte(b'S');
        self.u64(len as u64);
        Ok(StructWriter { c: self })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.byte(b'V');
        self.u64(variant_index as u64);
        self.u64(len as u64);
        Ok(StructVariantWriter { c: self })
    }
}

pub struct SeqWriter<'a> {
    c: &'a mut Canon,
}

impl<'a> SerializeSeq for SeqWriter<'a> {
    type Ok = ();
    type Error = CanonError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut *self.c)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a> SerializeTuple for SeqWriter<'a> {
    type Ok = ();
    type Error = CanonError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut *self.c)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a> SerializeTupleStruct for SeqWriter<'a> {
    type Ok = ();
    type Error = CanonError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut *self.c)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a> SerializeTupleVariant for SeqWriter<'a> {
    type Ok = ();
    type Error = CanonError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut *self.c)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

pub struct MapWriter<'a> {
    c: &'a mut Canon,
}

impl<'a> SerializeMap for MapWriter<'a> {
    type Ok = ();
    type Error = CanonError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        key.serialize(&mut *self.c)
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut *self.c)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

pub struct StructWriter<'a> {
    c: &'a mut Canon,
}

impl<'a> SerializeStruct for StructWriter<'a> {
    type Ok = ();
    type Error = CanonError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.c.byte(b's');
        self.c.text(key);
        value.serialize(&mut *self.c)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

pub struct StructVariantWriter<'a> {
    c: &'a mut Canon,
}

impl<'a> SerializeStructVariant for StructVariantWriter<'a> {
    type Ok = ();
    type Error = CanonError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.c.byte(b's');
        self.c.text(key);
        value.serialize(&mut *self.c)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::fingerprint;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    struct Sample {
        name: String,
        values: BTreeMap<String, i64>,
        maybe: Option<u64>,
        flag: bool,
    }

    fn sample() -> Sample {
        Sample {
            name: "x".into(),
            values: BTreeMap::from([("b".into(), 2), ("a".into(), 1)]),
            maybe: Some(9),
            flag: true,
        }
    }

    #[test]
    fn stable_and_value_sensitive() {
        let f1 = fingerprint(&sample());
        assert_eq!(f1, fingerprint(&sample()));
        let mut other = sample();
        other.flag = false;
        assert_ne!(f1, fingerprint(&other));
    }

    #[test]
    fn json_round_trip_preserves_fingerprint() {
        let bytes = serde_json::to_vec(&sample()).unwrap();
        let back: Sample = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(fingerprint(&sample()), fingerprint(&back));
    }
}
