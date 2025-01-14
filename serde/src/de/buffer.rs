//! Data structures for buffering self-describing formats.
//!
//! ```
//! # use serde::de::{Deserialize, value, IntoDeserializer, buffer::Buffer};
//! # let source = value::U64Deserializer::<value::Error>::new(32);
//! // Deserialize any self-describing format from `source` into `buffer`.
//! let buffer = Buffer::deserialize(source).unwrap();
//! // Turn the buffer back into a deserializer.
//! let deserializer = IntoDeserializer::<value::Error>::into_deserializer(buffer);
//! # assert_eq!(u32::deserialize(deserializer).unwrap(), 32);
//! ```

use crate::{actually_private, lib::*};

use crate::de::{
    self, size_hint, Deserialize, Deserializer, EnumAccess, Expected, MapAccess, SeqAccess, Visitor,
};

/// An efficient buffer for self-describing formats.
#[derive(Debug, Clone)]
pub struct Buffer<'de>(pub(crate) BufferInner<'de>);

impl<'de> Buffer<'de> {
    /// Tries to extract a string from the buffer.
    pub fn as_str(&self) -> Option<&str> {
        match self.0 {
            BufferInner::Str(x) => Some(x),
            BufferInner::String(ref x) => Some(x),
            BufferInner::Bytes(x) => str::from_utf8(x).ok(),
            BufferInner::ByteBuf(ref x) => str::from_utf8(x).ok(),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Buffer<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let visitor = BufferVisitor::new();
        deserializer.__deserialize_buffer(actually_private::T, visitor)
    }
}

impl<'de, E: de::Error> de::IntoDeserializer<'de, E> for Buffer<'de> {
    type Deserializer = BufferDeserializer<'de, E>;

    fn into_deserializer(self) -> Self::Deserializer {
        BufferDeserializer {
            buffer: self,
            err: PhantomData,
        }
    }
}

impl<'a, 'de, E: de::Error> de::IntoDeserializer<'de, E> for &'a Buffer<'de> {
    type Deserializer = BufferRefDeserializer<'a, 'de, E>;

    fn into_deserializer(self) -> Self::Deserializer {
        BufferRefDeserializer::new(self)
    }
}

macro_rules! impl_from_for_buffer {
    ($($type:ty => $constructor:ident ,)*) => {
        $(
            impl<'de> From<$type> for Buffer<'de> {
                fn from(value: $type) -> Self {
                    Buffer(BufferInner::$constructor(value))
                }
            }
        )*
    };
}

impl_from_for_buffer! {
    bool => Bool,

    u8 => U8,
    u16 => U16,
    u32 => U32,
    u64 => U64,

    i8 => I8,
    i16 => I16,
    i32 => I32,
    i64 => I64,

    f32 => F32,
    f64 => F64,

    char => Char,

    String => String,
    Vec<u8> => ByteBuf,
}

impl<'de> From<&'de str> for Buffer<'de> {
    fn from(value: &'de str) -> Self {
        Buffer(BufferInner::Str(value))
    }
}

impl<'de> From<&'de [u8]> for Buffer<'de> {
    fn from(value: &'de [u8]) -> Self {
        Buffer(BufferInner::Bytes(value))
    }
}

#[derive(Debug, Clone)]
pub(crate) enum BufferInner<'de> {
    Bool(bool),

    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    #[cfg(not(no_integer128))]
    U128(u128),

    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    #[cfg(not(no_integer128))]
    I128(i128),

    F32(f32),
    F64(f64),

    Char(char),
    String(String),
    Str(&'de str),
    ByteBuf(Vec<u8>),
    Bytes(&'de [u8]),

    None,
    Some(Box<Buffer<'de>>),

    Unit,
    Newtype(Box<Buffer<'de>>),
    Seq(Vec<Buffer<'de>>),
    Map(Vec<(Buffer<'de>, Buffer<'de>)>),
}

impl<'de> BufferInner<'de> {
    #[cold]
    fn unexpected(&self) -> de::Unexpected {
        match *self {
            BufferInner::Bool(b) => de::Unexpected::Bool(b),
            BufferInner::U8(n) => de::Unexpected::Unsigned(n as u64),
            BufferInner::U16(n) => de::Unexpected::Unsigned(n as u64),
            BufferInner::U32(n) => de::Unexpected::Unsigned(n as u64),
            BufferInner::U64(n) => de::Unexpected::Unsigned(n),
            BufferInner::I8(n) => de::Unexpected::Signed(n as i64),
            BufferInner::I16(n) => de::Unexpected::Signed(n as i64),
            BufferInner::I32(n) => de::Unexpected::Signed(n as i64),
            BufferInner::I64(n) => de::Unexpected::Signed(n),
            BufferInner::F32(f) => de::Unexpected::Float(f as f64),
            BufferInner::F64(f) => de::Unexpected::Float(f),
            BufferInner::Char(c) => de::Unexpected::Char(c),
            BufferInner::String(ref s) => de::Unexpected::Str(s),
            BufferInner::Str(s) => de::Unexpected::Str(s),
            BufferInner::ByteBuf(ref b) => de::Unexpected::Bytes(b),
            BufferInner::Bytes(b) => de::Unexpected::Bytes(b),
            BufferInner::None | BufferInner::Some(_) => de::Unexpected::Option,
            BufferInner::Unit => de::Unexpected::Unit,
            BufferInner::Newtype(_) => de::Unexpected::NewtypeStruct,
            BufferInner::Seq(_) => de::Unexpected::Seq,
            BufferInner::Map(_) => de::Unexpected::Map,
            #[cfg(not(no_integer128))]
            BufferInner::I128(_) | BufferInner::U128(_) => de::Unexpected::Other("128-bit integer"),
        }
    }
}

/// A [`Visitor`] for constructing [`Buffer`].
pub struct BufferVisitor<'de>(PhantomData<Buffer<'de>>);

impl<'de> BufferVisitor<'de> {
    /// Construct a new [`BufferVisitor`].
    pub fn new() -> Self {
        BufferVisitor(PhantomData)
    }
}

impl<'de> Visitor<'de> for BufferVisitor<'de> {
    type Value = Buffer<'de>;

    fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str("any value")
    }

    fn visit_bool<F>(self, value: bool) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::Bool(value)))
    }

    fn visit_i8<F>(self, value: i8) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::I8(value)))
    }

    fn visit_i16<F>(self, value: i16) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::I16(value)))
    }

    fn visit_i32<F>(self, value: i32) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::I32(value)))
    }

    fn visit_i64<F>(self, value: i64) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::I64(value)))
    }

    fn visit_u8<F>(self, value: u8) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::U8(value)))
    }

    fn visit_u16<F>(self, value: u16) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::U16(value)))
    }

    fn visit_u32<F>(self, value: u32) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::U32(value)))
    }

    fn visit_u64<F>(self, value: u64) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::U64(value)))
    }

    serde_if_integer128! {
        fn visit_i128<F>(self, value: i128) -> Result<Self::Value, F>
        where
            F: de::Error,
        {
            Ok(Buffer(BufferInner::I128(value)))
        }

        fn visit_u128<F>(self, value: u128) -> Result<Self::Value, F>
        where
            F: de::Error,
        {
            Ok(Buffer(BufferInner::U128(value)))
        }
    }

    fn visit_f32<F>(self, value: f32) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::F32(value)))
    }

    fn visit_f64<F>(self, value: f64) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::F64(value)))
    }

    fn visit_char<F>(self, value: char) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::Char(value)))
    }

    fn visit_str<F>(self, value: &str) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::String(value.into())))
    }

    fn visit_borrowed_str<F>(self, value: &'de str) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::Str(value)))
    }

    fn visit_string<F>(self, value: String) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::String(value)))
    }

    fn visit_bytes<F>(self, value: &[u8]) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::ByteBuf(value.into())))
    }

    fn visit_borrowed_bytes<F>(self, value: &'de [u8]) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::Bytes(value)))
    }

    fn visit_byte_buf<F>(self, value: Vec<u8>) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::ByteBuf(value)))
    }

    fn visit_unit<F>(self) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::Unit))
    }

    fn visit_none<F>(self) -> Result<Self::Value, F>
    where
        F: de::Error,
    {
        Ok(Buffer(BufferInner::None))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(|v| Buffer(BufferInner::Some(Box::new(v))))
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer).map(|v| Buffer(BufferInner::Newtype(Box::new(v))))
    }

    fn visit_seq<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
    where
        V: SeqAccess<'de>,
    {
        let mut vec = Vec::with_capacity(size_hint::cautious::<Buffer<'de>>(visitor.size_hint()));
        while let Some(e) = visitor.next_element()? {
            vec.push(e);
        }
        Ok(Buffer(BufferInner::Seq(vec)))
    }

    fn visit_map<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
    where
        V: MapAccess<'de>,
    {
        let mut vec = Vec::with_capacity(size_hint::cautious::<Buffer<'de>>(visitor.size_hint()));
        while let Some(kv) = visitor.next_entry()? {
            vec.push(kv);
        }
        Ok(Buffer(BufferInner::Map(vec)))
    }

    fn visit_enum<V>(self, _visitor: V) -> Result<Self::Value, V::Error>
    where
        V: EnumAccess<'de>,
    {
        Err(de::Error::custom(
            "untagged and internally tagged enums do not support enum input",
        ))
    }
}

/// A deserializer holding a [`Buffer`].
pub struct BufferDeserializer<'de, E> {
    buffer: Buffer<'de>,
    err: PhantomData<E>,
}

impl<'de, E> BufferDeserializer<'de, E>
where
    E: de::Error,
{
    #[cold]
    fn invalid_type(self, exp: &Expected) -> E {
        de::Error::invalid_type(self.buffer.0.unexpected(), exp)
    }

    fn deserialize_integer<V>(self, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::U8(v) => visitor.visit_u8(v),
            BufferInner::U16(v) => visitor.visit_u16(v),
            BufferInner::U32(v) => visitor.visit_u32(v),
            BufferInner::U64(v) => visitor.visit_u64(v),
            BufferInner::I8(v) => visitor.visit_i8(v),
            BufferInner::I16(v) => visitor.visit_i16(v),
            BufferInner::I32(v) => visitor.visit_i32(v),
            BufferInner::I64(v) => visitor.visit_i64(v),
            #[cfg(not(no_integer128))]
            BufferInner::U128(v) => visitor.visit_u128(v),
            #[cfg(not(no_integer128))]
            BufferInner::I128(v) => visitor.visit_i128(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_float<V>(self, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::F32(v) => visitor.visit_f32(v),
            BufferInner::F64(v) => visitor.visit_f64(v),
            BufferInner::U8(v) => visitor.visit_u8(v),
            BufferInner::U16(v) => visitor.visit_u16(v),
            BufferInner::U32(v) => visitor.visit_u32(v),
            BufferInner::U64(v) => visitor.visit_u64(v),
            BufferInner::I8(v) => visitor.visit_i8(v),
            BufferInner::I16(v) => visitor.visit_i16(v),
            BufferInner::I32(v) => visitor.visit_i32(v),
            BufferInner::I64(v) => visitor.visit_i64(v),
            #[cfg(not(no_integer128))]
            BufferInner::U128(v) => visitor.visit_u128(v),
            #[cfg(not(no_integer128))]
            BufferInner::I128(v) => visitor.visit_i128(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }
}

fn visit_buffer_seq<'de, V, E>(buffer: Vec<Buffer<'de>>, visitor: V) -> Result<V::Value, E>
where
    V: Visitor<'de>,
    E: de::Error,
{
    let seq = buffer.into_iter().map(BufferDeserializer::new);
    let mut seq_visitor = de::value::SeqDeserializer::new(seq);
    let value = visitor.visit_seq(&mut seq_visitor)?;
    seq_visitor.end()?;
    Ok(value)
}

fn visit_buffer_map<'de, V, E>(
    buffer: Vec<(Buffer<'de>, Buffer<'de>)>,
    visitor: V,
) -> Result<V::Value, E>
where
    V: Visitor<'de>,
    E: de::Error,
{
    let map = buffer
        .into_iter()
        .map(|(k, v)| (BufferDeserializer::new(k), BufferDeserializer::new(v)));
    let mut map_visitor = de::value::MapDeserializer::new(map);
    let value = visitor.visit_map(&mut map_visitor)?;
    map_visitor.end()?;
    Ok(value)
}

/// Used when deserializing an internally tagged enum because the buffer
/// will be used exactly once.
impl<'de, E> Deserializer<'de> for BufferDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::Bool(v) => visitor.visit_bool(v),
            BufferInner::U8(v) => visitor.visit_u8(v),
            BufferInner::U16(v) => visitor.visit_u16(v),
            BufferInner::U32(v) => visitor.visit_u32(v),
            BufferInner::U64(v) => visitor.visit_u64(v),
            BufferInner::I8(v) => visitor.visit_i8(v),
            BufferInner::I16(v) => visitor.visit_i16(v),
            BufferInner::I32(v) => visitor.visit_i32(v),
            BufferInner::I64(v) => visitor.visit_i64(v),
            BufferInner::F32(v) => visitor.visit_f32(v),
            BufferInner::F64(v) => visitor.visit_f64(v),
            BufferInner::Char(v) => visitor.visit_char(v),
            BufferInner::String(v) => visitor.visit_string(v),
            BufferInner::Str(v) => visitor.visit_borrowed_str(v),
            BufferInner::ByteBuf(v) => visitor.visit_byte_buf(v),
            BufferInner::Bytes(v) => visitor.visit_borrowed_bytes(v),
            BufferInner::Unit => visitor.visit_unit(),
            BufferInner::None => visitor.visit_none(),
            BufferInner::Some(v) => visitor.visit_some(BufferDeserializer::new(*v)),
            BufferInner::Newtype(v) => visitor.visit_newtype_struct(BufferDeserializer::new(*v)),
            BufferInner::Seq(v) => visit_buffer_seq(v, visitor),
            BufferInner::Map(v) => visit_buffer_map(v, visitor),
            #[cfg(not(no_integer128))]
            BufferInner::U128(v) => visitor.visit_u128(v),
            #[cfg(not(no_integer128))]
            BufferInner::I128(v) => visitor.visit_i128(v),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::Bool(v) => visitor.visit_bool(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    serde_if_integer128! {
        fn deserialize_i128<V>(self, visitor:V) -> Result<V::Value, Self::Error>
            where V:Visitor<'de>
        {
            self.deserialize_integer(visitor)
        }

        fn deserialize_u128<V>(self, visitor:V) -> Result<V::Value, Self::Error>
            where V:Visitor<'de>
        {
            self.deserialize_integer(visitor)
        }
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_float(visitor)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_float(visitor)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::Char(v) => visitor.visit_char(v),
            BufferInner::String(v) => visitor.visit_string(v),
            BufferInner::Str(v) => visitor.visit_borrowed_str(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::String(v) => visitor.visit_string(v),
            BufferInner::Str(v) => visitor.visit_borrowed_str(v),
            BufferInner::ByteBuf(v) => visitor.visit_byte_buf(v),
            BufferInner::Bytes(v) => visitor.visit_borrowed_bytes(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_byte_buf(visitor)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::String(v) => visitor.visit_string(v),
            BufferInner::Str(v) => visitor.visit_borrowed_str(v),
            BufferInner::ByteBuf(v) => visitor.visit_byte_buf(v),
            BufferInner::Bytes(v) => visitor.visit_borrowed_bytes(v),
            BufferInner::Seq(v) => visit_buffer_seq(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::None => visitor.visit_none(),
            BufferInner::Some(v) => visitor.visit_some(BufferDeserializer::new(*v)),
            BufferInner::Unit => visitor.visit_unit(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::Unit => visitor.visit_unit(),

            // Allow deserializing newtype variant containing unit.
            //
            //     #[derive(Deserialize)]
            //     #[serde(tag = "result")]
            //     enum Response<T> {
            //         Success(T),
            //     }
            //
            // We want {"result":"Success"} to deserialize into Response<()>.
            BufferInner::Map(ref v) if v.is_empty() => visitor.visit_unit(),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            // As a special case, allow deserializing untagged newtype
            // variant containing unit struct.
            //
            //     #[derive(Deserialize)]
            //     struct Info;
            //
            //     #[derive(Deserialize)]
            //     #[serde(tag = "topic")]
            //     enum Message {
            //         Info(Info),
            //     }
            //
            // We want {"topic":"Info"} to deserialize even though
            // ordinarily unit structs do not deserialize from empty map/seq.
            BufferInner::Map(ref v) if v.is_empty() => visitor.visit_unit(),
            BufferInner::Seq(ref v) if v.is_empty() => visitor.visit_unit(),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_newtype_struct<V>(self, _name: &str, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::Newtype(v) => visitor.visit_newtype_struct(BufferDeserializer::new(*v)),
            _ => visitor.visit_newtype_struct(self),
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::Seq(v) => visit_buffer_seq(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::Map(v) => visit_buffer_map(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::Seq(v) => visit_buffer_seq(v, visitor),
            BufferInner::Map(v) => visit_buffer_map(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let (variant, value) = match self.buffer.0 {
            BufferInner::Map(value) => {
                let mut iter = value.into_iter();
                let (variant, value) = match iter.next() {
                    Some(v) => v,
                    None => {
                        return Err(de::Error::invalid_value(
                            de::Unexpected::Map,
                            &"map with a single key",
                        ));
                    }
                };
                // enums are encoded in json as maps with a single key:value pair
                if iter.next().is_some() {
                    return Err(de::Error::invalid_value(
                        de::Unexpected::Map,
                        &"map with a single key",
                    ));
                }
                (variant.0, Some(value))
            }
            s @ BufferInner::String(_) | s @ BufferInner::Str(_) => (s, None),
            other => {
                return Err(de::Error::invalid_type(
                    other.unexpected(),
                    &"string or map",
                ));
            }
        };

        visitor.visit_enum(EnumDeserializer::new(Buffer(variant), value))
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.buffer.0 {
            BufferInner::String(v) => visitor.visit_string(v),
            BufferInner::Str(v) => visitor.visit_borrowed_str(v),
            BufferInner::ByteBuf(v) => visitor.visit_byte_buf(v),
            BufferInner::Bytes(v) => visitor.visit_borrowed_bytes(v),
            BufferInner::U8(v) => visitor.visit_u8(v),
            BufferInner::U64(v) => visitor.visit_u64(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        drop(self);
        visitor.visit_unit()
    }

    fn __deserialize_buffer<V>(
        self,
        _: actually_private::T,
        visitor: V,
    ) -> Result<Buffer<'de>, Self::Error>
    where
        V: Visitor<'de, Value = Buffer<'de>>,
    {
        let _ = visitor;
        Ok(self.buffer)
    }
}

impl<'de, E> BufferDeserializer<'de, E> {
    /// private API, don't use
    pub fn new(buffer: Buffer<'de>) -> Self {
        BufferDeserializer {
            buffer: buffer,
            err: PhantomData,
        }
    }
}

pub(crate) struct EnumDeserializer<'de, E>
where
    E: de::Error,
{
    variant: Buffer<'de>,
    value: Option<Buffer<'de>>,
    err: PhantomData<E>,
}

impl<'de, E> EnumDeserializer<'de, E>
where
    E: de::Error,
{
    pub(crate) fn new(
        variant: Buffer<'de>,
        value: Option<Buffer<'de>>,
    ) -> EnumDeserializer<'de, E> {
        EnumDeserializer {
            variant: variant,
            value: value,
            err: PhantomData,
        }
    }
}

impl<'de, E> de::EnumAccess<'de> for EnumDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;
    type Variant = VariantDeserializer<'de, Self::Error>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), E>
    where
        V: de::DeserializeSeed<'de>,
    {
        let visitor = VariantDeserializer {
            value: self.value,
            err: PhantomData,
        };
        seed.deserialize(BufferDeserializer::new(self.variant))
            .map(|v| (v, visitor))
    }
}

pub(crate) struct VariantDeserializer<'de, E>
where
    E: de::Error,
{
    value: Option<Buffer<'de>>,
    err: PhantomData<E>,
}

impl<'de, E> de::VariantAccess<'de> for VariantDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn unit_variant(self) -> Result<(), E> {
        match self.value {
            Some(value) => de::Deserialize::deserialize(BufferDeserializer::new(value)),
            None => Ok(()),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, E>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.value {
            Some(value) => seed.deserialize(BufferDeserializer::new(value)),
            None => Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"newtype variant",
            )),
        }
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Some(Buffer(BufferInner::Seq(v))) => {
                de::Deserializer::deserialize_any(SeqDeserializer::new(v), visitor)
            }
            Some(other) => Err(de::Error::invalid_type(
                other.0.unexpected(),
                &"tuple variant",
            )),
            None => Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"tuple variant",
            )),
        }
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Some(Buffer(BufferInner::Map(v))) => {
                de::Deserializer::deserialize_any(MapDeserializer::new(v), visitor)
            }
            Some(Buffer(BufferInner::Seq(v))) => {
                de::Deserializer::deserialize_any(SeqDeserializer::new(v), visitor)
            }
            Some(other) => Err(de::Error::invalid_type(
                other.0.unexpected(),
                &"struct variant",
            )),
            None => Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"struct variant",
            )),
        }
    }
}

struct SeqDeserializer<'de, E>
where
    E: de::Error,
{
    iter: <Vec<Buffer<'de>> as IntoIterator>::IntoIter,
    err: PhantomData<E>,
}

impl<'de, E> SeqDeserializer<'de, E>
where
    E: de::Error,
{
    fn new(vec: Vec<Buffer<'de>>) -> Self {
        SeqDeserializer {
            iter: vec.into_iter(),
            err: PhantomData,
        }
    }
}

impl<'de, E> de::Deserializer<'de> for SeqDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    #[inline]
    fn deserialize_any<V>(mut self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let len = self.iter.len();
        if len == 0 {
            visitor.visit_unit()
        } else {
            let ret = visitor.visit_seq(&mut self)?;
            let remaining = self.iter.len();
            if remaining == 0 {
                Ok(ret)
            } else {
                Err(de::Error::invalid_length(len, &"fewer elements in array"))
            }
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'de, E> de::SeqAccess<'de> for SeqDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => seed.deserialize(BufferDeserializer::new(value)).map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        size_hint::from_bounds(&self.iter)
    }
}

struct MapDeserializer<'de, E>
where
    E: de::Error,
{
    iter: <Vec<(Buffer<'de>, Buffer<'de>)> as IntoIterator>::IntoIter,
    value: Option<Buffer<'de>>,
    err: PhantomData<E>,
}

impl<'de, E> MapDeserializer<'de, E>
where
    E: de::Error,
{
    fn new(map: Vec<(Buffer<'de>, Buffer<'de>)>) -> Self {
        MapDeserializer {
            iter: map.into_iter(),
            value: None,
            err: PhantomData,
        }
    }
}

impl<'de, E> de::MapAccess<'de> for MapDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn next_key_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                seed.deserialize(BufferDeserializer::new(key)).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.value.take() {
            Some(value) => seed.deserialize(BufferDeserializer::new(value)),
            None => Err(de::Error::custom("value is missing")),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        size_hint::from_bounds(&self.iter)
    }
}

impl<'de, E> de::Deserializer<'de> for MapDeserializer<'de, E>
where
    E: de::Error,
{
    type Error = E;

    #[inline]
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_map(self)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

/// A deserializer holding a [`Buffer`] reference.
pub struct BufferRefDeserializer<'a, 'de: 'a, E> {
    buffer: &'a BufferInner<'de>,
    err: PhantomData<E>,
}

impl<'a, 'de, E> BufferRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    #[cold]
    fn invalid_type(self, exp: &Expected) -> E {
        de::Error::invalid_type(self.buffer.unexpected(), exp)
    }

    fn deserialize_integer<V>(self, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::U8(v) => visitor.visit_u8(v),
            BufferInner::U16(v) => visitor.visit_u16(v),
            BufferInner::U32(v) => visitor.visit_u32(v),
            BufferInner::U64(v) => visitor.visit_u64(v),
            BufferInner::I8(v) => visitor.visit_i8(v),
            BufferInner::I16(v) => visitor.visit_i16(v),
            BufferInner::I32(v) => visitor.visit_i32(v),
            BufferInner::I64(v) => visitor.visit_i64(v),
            #[cfg(not(no_integer128))]
            BufferInner::U128(v) => visitor.visit_u128(v),
            #[cfg(not(no_integer128))]
            BufferInner::I128(v) => visitor.visit_i128(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_float<V>(self, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::F32(v) => visitor.visit_f32(v),
            BufferInner::F64(v) => visitor.visit_f64(v),
            BufferInner::U8(v) => visitor.visit_u8(v),
            BufferInner::U16(v) => visitor.visit_u16(v),
            BufferInner::U32(v) => visitor.visit_u32(v),
            BufferInner::U64(v) => visitor.visit_u64(v),
            BufferInner::I8(v) => visitor.visit_i8(v),
            BufferInner::I16(v) => visitor.visit_i16(v),
            BufferInner::I32(v) => visitor.visit_i32(v),
            BufferInner::I64(v) => visitor.visit_i64(v),
            #[cfg(not(no_integer128))]
            BufferInner::U128(v) => visitor.visit_u128(v),
            #[cfg(not(no_integer128))]
            BufferInner::I128(v) => visitor.visit_i128(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }
}

fn visit_buffer_seq_ref<'a, 'de, V, E>(buffer: &'a [Buffer<'de>], visitor: V) -> Result<V::Value, E>
where
    V: Visitor<'de>,
    E: de::Error,
{
    let seq = buffer.iter().map(BufferRefDeserializer::new);
    let mut seq_visitor = de::value::SeqDeserializer::new(seq);
    let value = visitor.visit_seq(&mut seq_visitor)?;
    seq_visitor.end()?;
    Ok(value)
}

fn visit_buffer_map_ref<'a, 'de, V, E>(
    buffer: &'a [(Buffer<'de>, Buffer<'de>)],
    visitor: V,
) -> Result<V::Value, E>
where
    V: Visitor<'de>,
    E: de::Error,
{
    let map = buffer.iter().map(|entry| {
        (
            BufferRefDeserializer::new(&entry.0),
            BufferRefDeserializer::new(&entry.1),
        )
    });
    let mut map_visitor = de::value::MapDeserializer::new(map);
    let value = visitor.visit_map(&mut map_visitor)?;
    map_visitor.end()?;
    Ok(value)
}

/// Used when deserializing an untagged enum because the buffer may need
/// to be used more than once.
impl<'de, 'a, E> Deserializer<'de> for BufferRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::Bool(v) => visitor.visit_bool(v),
            BufferInner::U8(v) => visitor.visit_u8(v),
            BufferInner::U16(v) => visitor.visit_u16(v),
            BufferInner::U32(v) => visitor.visit_u32(v),
            BufferInner::U64(v) => visitor.visit_u64(v),
            BufferInner::I8(v) => visitor.visit_i8(v),
            BufferInner::I16(v) => visitor.visit_i16(v),
            BufferInner::I32(v) => visitor.visit_i32(v),
            BufferInner::I64(v) => visitor.visit_i64(v),
            BufferInner::F32(v) => visitor.visit_f32(v),
            BufferInner::F64(v) => visitor.visit_f64(v),
            BufferInner::Char(v) => visitor.visit_char(v),
            BufferInner::String(ref v) => visitor.visit_str(v),
            BufferInner::Str(v) => visitor.visit_borrowed_str(v),
            BufferInner::ByteBuf(ref v) => visitor.visit_bytes(v),
            BufferInner::Bytes(v) => visitor.visit_borrowed_bytes(v),
            BufferInner::Unit => visitor.visit_unit(),
            BufferInner::None => visitor.visit_none(),
            BufferInner::Some(ref v) => visitor.visit_some(BufferRefDeserializer::new(v)),
            BufferInner::Newtype(ref v) => {
                visitor.visit_newtype_struct(BufferRefDeserializer::new(v))
            }
            BufferInner::Seq(ref v) => visit_buffer_seq_ref(v, visitor),
            BufferInner::Map(ref v) => visit_buffer_map_ref(v, visitor),
            #[cfg(not(no_integer128))]
            BufferInner::U128(v) => visitor.visit_u128(v),
            #[cfg(not(no_integer128))]
            BufferInner::I128(v) => visitor.visit_i128(v),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::Bool(v) => visitor.visit_bool(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_integer(visitor)
    }

    serde_if_integer128! {
        fn deserialize_i128<V>(self, visitor:V) -> Result<V::Value, Self::Error>
            where V:Visitor<'de>
        {
            self.deserialize_integer(visitor)
        }

        fn deserialize_u128<V>(self, visitor:V) -> Result<V::Value, Self::Error>
            where V:Visitor<'de>
        {
            self.deserialize_integer(visitor)
        }
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_float(visitor)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_float(visitor)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::Char(v) => visitor.visit_char(v),
            BufferInner::String(ref v) => visitor.visit_str(v),
            BufferInner::Str(v) => visitor.visit_borrowed_str(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::String(ref v) => visitor.visit_str(v),
            BufferInner::Str(v) => visitor.visit_borrowed_str(v),
            BufferInner::ByteBuf(ref v) => visitor.visit_bytes(v),
            BufferInner::Bytes(v) => visitor.visit_borrowed_bytes(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::String(ref v) => visitor.visit_str(v),
            BufferInner::Str(v) => visitor.visit_borrowed_str(v),
            BufferInner::ByteBuf(ref v) => visitor.visit_bytes(v),
            BufferInner::Bytes(v) => visitor.visit_borrowed_bytes(v),
            BufferInner::Seq(ref v) => visit_buffer_seq_ref(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::None => visitor.visit_none(),
            BufferInner::Some(ref v) => visitor.visit_some(BufferRefDeserializer::new(v)),
            BufferInner::Unit => visitor.visit_unit(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::Unit => visitor.visit_unit(),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(self, _name: &str, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::Newtype(ref v) => {
                visitor.visit_newtype_struct(BufferRefDeserializer::new(v))
            }
            _ => visitor.visit_newtype_struct(self),
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::Seq(ref v) => visit_buffer_seq_ref(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::Map(ref v) => visit_buffer_map_ref(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::Seq(ref v) => visit_buffer_seq_ref(v, visitor),
            BufferInner::Map(ref v) => visit_buffer_map_ref(v, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let (variant, value) = match *self.buffer {
            BufferInner::Map(ref value) => {
                let mut iter = value.iter();
                let &(ref variant, ref value) = match iter.next() {
                    Some(v) => v,
                    None => {
                        return Err(de::Error::invalid_value(
                            de::Unexpected::Map,
                            &"map with a single key",
                        ));
                    }
                };
                // enums are encoded in json as maps with a single key:value pair
                if iter.next().is_some() {
                    return Err(de::Error::invalid_value(
                        de::Unexpected::Map,
                        &"map with a single key",
                    ));
                }
                (&variant.0, Some(&value.0))
            }
            ref s @ BufferInner::String(_) | ref s @ BufferInner::Str(_) => (s, None),
            ref other => {
                return Err(de::Error::invalid_type(
                    other.unexpected(),
                    &"string or map",
                ));
            }
        };

        visitor.visit_enum(EnumRefDeserializer {
            variant: variant,
            value: value,
            err: PhantomData,
        })
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match *self.buffer {
            BufferInner::String(ref v) => visitor.visit_str(v),
            BufferInner::Str(v) => visitor.visit_borrowed_str(v),
            BufferInner::ByteBuf(ref v) => visitor.visit_bytes(v),
            BufferInner::Bytes(v) => visitor.visit_borrowed_bytes(v),
            BufferInner::U8(v) => visitor.visit_u8(v),
            BufferInner::U64(v) => visitor.visit_u64(v),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn __deserialize_buffer<V>(
        self,
        _: actually_private::T,
        visitor: V,
    ) -> Result<Buffer<'de>, Self::Error>
    where
        V: Visitor<'de, Value = Buffer<'de>>,
    {
        let _ = visitor;
        Ok(Buffer(self.buffer.clone()))
    }
}

impl<'a, 'de, E> BufferRefDeserializer<'a, 'de, E> {
    /// Constructs a new [`BufferRefDeserializer`].
    pub fn new(buffer: &'a Buffer<'de>) -> Self {
        Self::new_inner(&buffer.0)
    }

    /// Construct a new [`BufferRefDeserializer`] from a reference to [`BufferInner`].
    fn new_inner(buffer: &'a BufferInner<'de>) -> Self {
        BufferRefDeserializer {
            buffer: buffer,
            err: PhantomData,
        }
    }
}

struct EnumRefDeserializer<'a, 'de: 'a, E>
where
    E: de::Error,
{
    variant: &'a BufferInner<'de>,
    value: Option<&'a BufferInner<'de>>,
    err: PhantomData<E>,
}

impl<'de, 'a, E> de::EnumAccess<'de> for EnumRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    type Error = E;
    type Variant = VariantRefDeserializer<'a, 'de, Self::Error>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        let visitor = VariantRefDeserializer {
            value: self.value,
            err: PhantomData,
        };
        seed.deserialize(BufferRefDeserializer::new_inner(self.variant))
            .map(|v| (v, visitor))
    }
}

struct VariantRefDeserializer<'a, 'de: 'a, E>
where
    E: de::Error,
{
    value: Option<&'a BufferInner<'de>>,
    err: PhantomData<E>,
}

impl<'de, 'a, E> de::VariantAccess<'de> for VariantRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn unit_variant(self) -> Result<(), E> {
        match self.value {
            Some(value) => de::Deserialize::deserialize(BufferRefDeserializer::new_inner(value)),
            None => Ok(()),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, E>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.value {
            Some(value) => seed.deserialize(BufferRefDeserializer::new_inner(value)),
            None => Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"newtype variant",
            )),
        }
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Some(&BufferInner::Seq(ref v)) => {
                de::Deserializer::deserialize_any(SeqRefDeserializer::new(v), visitor)
            }
            Some(other) => Err(de::Error::invalid_type(
                other.unexpected(),
                &"tuple variant",
            )),
            None => Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"tuple variant",
            )),
        }
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Some(&BufferInner::Map(ref v)) => {
                de::Deserializer::deserialize_any(MapRefDeserializer::new(&v), visitor)
            }
            Some(&BufferInner::Seq(ref v)) => {
                de::Deserializer::deserialize_any(SeqRefDeserializer::new(&v), visitor)
            }
            Some(other) => Err(de::Error::invalid_type(
                other.unexpected(),
                &"struct variant",
            )),
            None => Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"struct variant",
            )),
        }
    }
}

struct SeqRefDeserializer<'a, 'de: 'a, E>
where
    E: de::Error,
{
    iter: <&'a [Buffer<'de>] as IntoIterator>::IntoIter,
    err: PhantomData<E>,
}

impl<'a, 'de, E> SeqRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    fn new(slice: &'a [Buffer<'de>]) -> Self {
        SeqRefDeserializer {
            iter: slice.iter(),
            err: PhantomData,
        }
    }
}

impl<'de, 'a, E> de::Deserializer<'de> for SeqRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    type Error = E;

    #[inline]
    fn deserialize_any<V>(mut self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        let len = self.iter.len();
        if len == 0 {
            visitor.visit_unit()
        } else {
            let ret = visitor.visit_seq(&mut self)?;
            let remaining = self.iter.len();
            if remaining == 0 {
                Ok(ret)
            } else {
                Err(de::Error::invalid_length(len, &"fewer elements in array"))
            }
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'de, 'a, E> de::SeqAccess<'de> for SeqRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => seed
                .deserialize(BufferRefDeserializer::new(value))
                .map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        size_hint::from_bounds(&self.iter)
    }
}

struct MapRefDeserializer<'a, 'de: 'a, E>
where
    E: de::Error,
{
    iter: <&'a [(Buffer<'de>, Buffer<'de>)] as IntoIterator>::IntoIter,
    value: Option<&'a Buffer<'de>>,
    err: PhantomData<E>,
}

impl<'a, 'de, E> MapRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    fn new(map: &'a [(Buffer<'de>, Buffer<'de>)]) -> Self {
        MapRefDeserializer {
            iter: map.iter(),
            value: None,
            err: PhantomData,
        }
    }
}

impl<'de, 'a, E> de::MapAccess<'de> for MapRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn next_key_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(&(ref key, ref value)) => {
                self.value = Some(value);
                seed.deserialize(BufferRefDeserializer::new(key)).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.value.take() {
            Some(value) => seed.deserialize(BufferRefDeserializer::new(value)),
            None => Err(de::Error::custom("value is missing")),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        size_hint::from_bounds(&self.iter)
    }
}

impl<'de, 'a, E> de::Deserializer<'de> for MapRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    type Error = E;

    #[inline]
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_map(self)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'de, E> de::IntoDeserializer<'de, E> for BufferDeserializer<'de, E>
where
    E: de::Error,
{
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de, 'a, E> de::IntoDeserializer<'de, E> for BufferRefDeserializer<'a, 'de, E>
where
    E: de::Error,
{
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}
