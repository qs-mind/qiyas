//! Exact, temporary cache input bytes. This is not a file or wire format. Type tags,
//! lengths and end markers separate values; floats retain all bits and strings are
//! copied verbatim, avoiding repeated JSON escaping of drawing-owned knowledge.
use serde::{ser, Serialize};
use std::io::Write;

type Error = serde_json::Error;
type Result = std::result::Result<(), Error>;

pub(super) fn write(output: impl Write, value: &impl Serialize) -> Result {
    value.serialize(&mut Encoder(output))
}
struct Encoder<W>(W);
struct Compound<'a, W>(&'a mut Encoder<W>);
impl<W: Write> Encoder<W> {
    fn raw(&mut self, bytes: &[u8]) -> Result {
        self.0.write_all(bytes).map_err(ser::Error::custom)
    }
    fn tag(&mut self, tag: u8) -> Result {
        self.raw(&[tag])
    }
    fn length(&mut self, length: usize) -> Result {
        self.raw(&(length as u64).to_le_bytes())
    }
    fn bytes(&mut self, tag: u8, bytes: &[u8]) -> Result {
        self.tag(tag)?;
        self.length(bytes.len())?;
        self.raw(bytes)
    }
    fn variant(&mut self, tag: u8, name: &str, index: u32, variant: &str) -> Result {
        self.tag(tag)?;
        self.bytes(22, name.as_bytes())?;
        self.raw(&index.to_le_bytes())?;
        self.bytes(22, variant.as_bytes())
    }
    fn container(&mut self, tag: u8, len: Option<usize>) -> Result {
        self.tag(tag)?;
        self.tag(u8::from(len.is_some()))?;
        self.length(len.unwrap_or_default())
    }
}
macro_rules! integer {
    ($method:ident, $kind:ty, $tag:expr) => {
        fn $method(self, value: $kind) -> Result {
            self.tag($tag)?;
            self.raw(&value.to_le_bytes())
        }
    };
}
impl<'a, W: Write> ser::Serializer for &'a mut Encoder<W> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Compound<'a, W>;
    type SerializeTuple = Compound<'a, W>;
    type SerializeTupleStruct = Compound<'a, W>;
    type SerializeTupleVariant = Compound<'a, W>;
    type SerializeMap = Compound<'a, W>;
    type SerializeStruct = Compound<'a, W>;
    type SerializeStructVariant = Compound<'a, W>;

    fn serialize_bool(self, value: bool) -> Result {
        self.raw(&[1, u8::from(value)])
    }
    integer!(serialize_i8, i8, 2);
    integer!(serialize_i16, i16, 3);
    integer!(serialize_i32, i32, 4);
    integer!(serialize_i64, i64, 5);
    integer!(serialize_i128, i128, 6);
    integer!(serialize_u8, u8, 7);
    integer!(serialize_u16, u16, 8);
    integer!(serialize_u32, u32, 9);
    integer!(serialize_u64, u64, 10);
    integer!(serialize_u128, u128, 11);
    fn serialize_f32(self, value: f32) -> Result {
        self.tag(12)?;
        self.raw(&value.to_bits().to_le_bytes())
    }
    fn serialize_f64(self, value: f64) -> Result {
        self.tag(13)?;
        self.raw(&value.to_bits().to_le_bytes())
    }
    fn serialize_char(self, value: char) -> Result {
        self.tag(14)?;
        self.raw(&(value as u32).to_le_bytes())
    }
    fn serialize_str(self, value: &str) -> Result {
        self.bytes(22, value.as_bytes())
    }
    fn serialize_bytes(self, value: &[u8]) -> Result {
        self.bytes(23, value)
    }
    fn serialize_none(self) -> Result {
        self.tag(24)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result {
        self.tag(25)?;
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result {
        self.tag(26)
    }
    fn serialize_unit_struct(self, name: &'static str) -> Result {
        self.bytes(27, name.as_bytes())
    }
    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> Result {
        self.variant(28, name, index, variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result {
        self.bytes(29, name.as_bytes())?;
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result {
        self.variant(30, name, index, variant)?;
        value.serialize(self)
    }
    fn serialize_seq(self, len: Option<usize>) -> std::result::Result<Self::SerializeSeq, Error> {
        self.container(31, len)?;
        Ok(Compound(self))
    }
    fn serialize_tuple(self, len: usize) -> std::result::Result<Self::SerializeTuple, Error> {
        self.container(32, Some(len))?;
        Ok(Compound(self))
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> std::result::Result<Self::SerializeTupleStruct, Error> {
        self.bytes(33, name.as_bytes())?;
        self.length(len)?;
        Ok(Compound(self))
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> std::result::Result<Self::SerializeTupleVariant, Error> {
        self.variant(34, name, index, variant)?;
        self.length(len)?;
        Ok(Compound(self))
    }
    fn serialize_map(self, len: Option<usize>) -> std::result::Result<Self::SerializeMap, Error> {
        self.container(35, len)?;
        Ok(Compound(self))
    }
    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> std::result::Result<Self::SerializeStruct, Error> {
        self.bytes(36, name.as_bytes())?;
        self.length(len)?;
        Ok(Compound(self))
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> std::result::Result<Self::SerializeStructVariant, Error> {
        self.variant(37, name, index, variant)?;
        self.length(len)?;
        Ok(Compound(self))
    }
}
macro_rules! sequence {
    ($kind:ident, $method:ident) => {
        impl<W: Write> ser::$kind for Compound<'_, W> {
            type Ok = ();
            type Error = Error;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result {
                value.serialize(&mut *self.0)
            }
            fn end(self) -> Result {
                self.0.tag(255)
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);
sequence!(SerializeTupleVariant, serialize_field);
impl<W: Write> ser::SerializeMap for Compound<'_, W> {
    type Ok = ();
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, value: &T) -> Result {
        value.serialize(&mut *self.0)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result {
        value.serialize(&mut *self.0)
    }
    fn end(self) -> Result {
        self.0.tag(255)
    }
}
macro_rules! structure {
    ($kind:ident) => {
        impl<W: Write> ser::$kind for Compound<'_, W> {
            type Ok = ();
            type Error = Error;
            fn serialize_field<T: Serialize + ?Sized>(
                &mut self,
                key: &'static str,
                value: &T,
            ) -> Result {
                self.0.bytes(22, key.as_bytes())?;
                value.serialize(&mut *self.0)
            }
            fn end(self) -> Result {
                self.0.tag(255)
            }
        }
    };
}
structure!(SerializeStruct);
structure!(SerializeStructVariant);

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(value: &impl Serialize) -> Vec<u8> {
        let mut out = Vec::new();
        write(&mut out, value).unwrap();
        out
    }
    #[test]
    fn changed_number_bits_strings_and_container_boundaries_cannot_alias() {
        assert_ne!(bytes(&0.0f64), bytes(&-0.0f64));
        assert_ne!(
            bytes(&f64::from_bits(0x7ff8_0000_0000_0001)),
            bytes(&f64::from_bits(0x7ff8_0000_0000_0002))
        );
        assert_ne!(bytes(&1u64), bytes(&1.0f64));
        assert_ne!(bytes(&["ab", "c"]), bytes(&["a", "bc"]));
        assert_ne!(
            bytes(&vec![vec![1u8], vec![2, 3]]),
            bytes(&vec![vec![1u8, 2], vec![3]])
        );
        assert_ne!(bytes(&Some("null")), bytes(&Option::<&str>::None));
        let knowledge = serde_json::json!({"expression":"a + lookup(\"dimensions\", key, \"width\")", "text":"عربي\n\u{0}\\\""});
        assert_eq!(bytes(&knowledge), bytes(&knowledge.clone()));
        let mut changed = knowledge.clone();
        changed["text"] = serde_json::json!("عربي\n\u{0}\\");
        assert_ne!(bytes(&knowledge), bytes(&changed));
    }
}
