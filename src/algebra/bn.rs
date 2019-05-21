use super::groups::{Group};
use super::pairing::Pairing;
use rand::{CryptoRng, Rng};
use digest::Digest;
use digest::generic_array::typenum::U64;
use crate::utils::u8_bigendian_slice_to_u32;
use std::fmt;
use bn::{Group as BNGroup};
use serde::de::{SeqAccess, Visitor};
use serde::{Deserializer, Serializer, Serialize, Deserialize};
use crate::algebra::groups::Scalar;

pub struct BNScalar(pub(crate) bn::Fr);
pub struct BNG1(pub(crate) bn::G1);
pub struct BNG2(pub(crate) bn::G2);
#[derive(Clone, PartialEq, Eq)]
pub struct BNGt(pub(crate) bn::Gt);

impl fmt::Debug for BNScalar{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Fr:{}", rustc_serialize::json::encode(&self.0).unwrap())
    }
}

impl PartialEq for BNScalar{
    fn eq(&self, other: &BNScalar) -> bool{
        self.0 == other.0
    }
}

impl Eq for BNScalar {}

impl Clone for BNScalar {
    fn clone(&self) -> BNScalar{
        BNScalar(self.0.clone())
    }
}

impl crate::algebra::groups::Scalar for BNScalar {
    // scalar generation
    fn random_scalar<R: CryptoRng + Rng>(rng: &mut R) -> BNScalar{
        // hack to use rand_04::Rng rather than rand::Rng
        let mut random_bytes = [0u8;16];
        rng.fill_bytes(&mut random_bytes);
        let mut seed = [0u32;4];
        for i in 0..4{
            seed[i] = u8_bigendian_slice_to_u32(&random_bytes[i*4..(i+1)*4]);
        }

        use rand_04::SeedableRng;
        let mut prng_04 = rand_04::ChaChaRng::from_seed(&seed);
        BNScalar(bn::Fr::random(&mut prng_04))
    }

    fn from_u32(value: u32) -> BNScalar{
        Self::from_u64(value as u64)
    }

    fn from_u64(value: u64) -> BNScalar {
        let mut v  = value;
        let two = bn::Fr::one() + bn::Fr::one();
        let mut result = bn::Fr::zero();
        let mut two_pow_i = bn::Fr::one();
        for _ in 0..64{
            if v == 0 {break;}
            if v&1 == 1u64 {
                result = result + two_pow_i;
            }
            v = v>>1;
            two_pow_i = two_pow_i * two;
        }
        BNScalar(result)
    }

    fn from_hash<D>(hash: D) -> BNScalar
        where D: Digest<OutputSize = U64> + Default{
        let result = hash.result();
        let mut seed = [0u32; 16];
        for i in 0..16{
            seed[i] = u8_bigendian_slice_to_u32(&result.as_slice()[i*4..(i+1)*4]);
        }
        use rand_04::SeedableRng;
        let mut prng = rand_04::ChaChaRng::from_seed(&seed);
        BNScalar(bn::Fr::random(&mut prng))
    }

    // scalar arithmetic
    fn add(&self, b: &BNScalar) -> BNScalar{
        BNScalar(self.0 + b.0)
    }
    fn mul(&self, b: &BNScalar) -> BNScalar{
        BNScalar(self.0 * b.0)
    }

    //scalar serialization
    fn to_bytes(&self) -> Vec<u8>{
        let json = rustc_serialize::json::encode(&self.0).unwrap();
        let bytes = json.into_bytes();
        bytes

    }
    fn from_bytes(bytes: &[u8]) -> BNScalar {
        let json = &String::from_utf8(bytes.to_vec()).unwrap();
        BNScalar(rustc_serialize::json::decode(json).unwrap())
    }
}

impl Serialize for BNScalar {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer
    {
        serializer.serialize_bytes(self.to_bytes().as_slice())
    }
}

impl<'de> Deserialize<'de> for BNScalar {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>
    {
        struct ScalarVisitor;

        impl<'de> Visitor<'de> for ScalarVisitor{
            type Value = BNScalar;

            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                formatter.write_str("a encoded BLSG2 element")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<BNScalar, E>
                where E: serde::de::Error
            {
                Ok(BNScalar::from_bytes(v))
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<BNScalar, V::Error>
                where V: SeqAccess<'de>,
            {
                let mut vec: Vec<u8> = vec![];
                while let Some(x) = seq.next_element().unwrap() {
                    vec.push(x);
                }
                Ok(BNScalar::from_bytes(vec.as_slice()))
            }
        }
        deserializer.deserialize_bytes(ScalarVisitor)
    }
}

impl fmt::Debug for BNG1{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Fr:{}", rustc_serialize::json::encode(&self.0).unwrap())
    }
}

impl PartialEq for BNG1{
    fn eq(&self, other: &BNG1) -> bool{
        self.0 == other.0
    }
}

impl Eq for BNG1 {}

impl Clone for BNG1 {
    fn clone(&self) -> BNG1{
        BNG1(self.0.clone())
    }
}



impl Group for BNG1{
    type ScalarType = BNScalar;
    const COMPRESSED_LEN: usize = 0; // TODO
    const SCALAR_BYTES_LEN: usize = 0; // TODO
    fn get_identity() -> BNG1{
        BNG1(bn::G1::zero())
    }
    fn get_base() -> BNG1{
        BNG1(bn::G1::one())
    }

    // compression/serialization helpers
    fn to_compressed_bytes(&self) -> Vec<u8>{
        rustc_serialize::json::encode(&self.0).unwrap().into_bytes()
    }
    fn from_compressed_bytes(bytes: &[u8]) -> Option<BNG1>{
        let json = &String::from_utf8(bytes.to_vec()).unwrap();
        match rustc_serialize::json::decode(json){
            Ok(x) => Some(BNG1(x)),
            Err(_) => None,
        }
    }

    //arithmetic
    fn mul(&self, scalar: &BNScalar) -> BNG1 {
        return BNG1(self.0 * scalar.0)
    }
    fn add(&self, other: &Self) -> BNG1{
        BNG1(self.0 + other.0)
    }
    fn sub(&self, other: &Self) -> BNG1{
        BNG1(self.0 - other.0)
    }
}


impl Serialize for BNG1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer
    {
        serializer.serialize_bytes(self.to_compressed_bytes().as_slice())
    }
}

impl<'de> Deserialize<'de> for BNG1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>
    {
        struct G1Visitor;

        impl<'de> Visitor<'de> for G1Visitor{
            type Value = BNG1;

            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                formatter.write_str("a encoded BLSG2 element")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<BNG1, E>
                where E: serde::de::Error
            {
                Ok(BNG1::from_compressed_bytes(v).unwrap()) //TODO handle error
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<BNG1, V::Error>
                where V: SeqAccess<'de>,
            {
                let mut vec: Vec<u8> = vec![];
                while let Some(x) = seq.next_element().unwrap() {
                    vec.push(x);
                }
                Ok(BNG1::from_compressed_bytes(vec.as_slice()).unwrap())
            }
        }
        deserializer.deserialize_bytes(G1Visitor)
    }
}


impl fmt::Debug for BNG2{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Fr:{}", rustc_serialize::json::encode(&self.0).unwrap())
    }
}

impl PartialEq for BNG2{
    fn eq(&self, other: &BNG2) -> bool{
        self.0 == other.0
    }
}

impl Eq for BNG2 {}

impl Clone for BNG2 {
    fn clone(&self) -> BNG2{
        BNG2(self.0.clone())
    }
}



impl Group for BNG2{
    type ScalarType = BNScalar;
    const COMPRESSED_LEN: usize = 0; // TODO
    const SCALAR_BYTES_LEN: usize = 0; // TODO
    fn get_identity() -> BNG2{
        BNG2(bn::G2::zero())
    }
    fn get_base() -> BNG2{
        BNG2(bn::G2::one())
    }

    // compression/serialization helpers
    fn to_compressed_bytes(&self) -> Vec<u8>{
        rustc_serialize::json::encode(&self.0).unwrap().into_bytes()
    }
    fn from_compressed_bytes(bytes: &[u8]) -> Option<BNG2>{
        let json = &String::from_utf8(bytes.to_vec()).unwrap();
        match rustc_serialize::json::decode(json){
            Ok(x) => Some(BNG2(x)),
            Err(_) => None,
        }
    }

    //arithmetic
    fn mul(&self, scalar: &BNScalar) -> BNG2 {
        return BNG2(self.0 * scalar.0)
    }
    fn add(&self, other: &Self) -> BNG2{
        BNG2(self.0 + other.0)
    }
    fn sub(&self, other: &Self) -> BNG2{
        BNG2(self.0 - other.0)
    }
}

impl Serialize for BNG2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer
    {
        serializer.serialize_bytes(self.to_compressed_bytes().as_slice())
    }
}

impl<'de> Deserialize<'de> for BNG2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>
    {
        struct G2Visitor;

        impl<'de> Visitor<'de> for G2Visitor{
            type Value = BNG2;

            fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                formatter.write_str("a encoded BLSG2 element")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<BNG2, E>
                where E: serde::de::Error
            {
                Ok(BNG2::from_compressed_bytes(v).unwrap()) //TODO handle error
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<BNG2, V::Error>
                where V: SeqAccess<'de>,
            {
                let mut vec: Vec<u8> = vec![];
                while let Some(x) = seq.next_element().unwrap() {
                    vec.push(x);
                }
                Ok(BNG2::from_compressed_bytes(vec.as_slice()).unwrap())
            }
        }
        deserializer.deserialize_bytes(G2Visitor)
    }
}

impl fmt::Debug for BNGt{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Fr: Some Gt Element")
    }
}

impl Pairing for BNGt {
    type G1 = BNG1;
    type G2 = BNG2;
    type ScalarType = BNScalar;

    fn pairing(a: &Self::G1, b: &Self::G2) -> BNGt{
        BNGt(bn::pairing(a.0, b.0))
    }
    fn scalar_mul(&self, a: &Self::ScalarType) -> BNGt{
        BNGt(self.0.pow(a.0))
    }
    fn add(&self, other: &Self) -> BNGt{
        BNGt(self.0 * other.0)
    }

    fn g1_mul_scalar(a: &Self::G1, b: &Self::ScalarType) -> Self::G1{
        a.mul(b)
    }
    fn g2_mul_scalar(a: &Self::G2, b: &Self::ScalarType) -> Self::G2{
        a.mul(b)
    }
}


#[cfg(test)]
mod bn_groups_test{
    use crate::algebra::groups::group_tests::{test_scalar_operations};

    #[test]
    fn scalar_ops(){
        test_scalar_operations::<super::BNScalar>();
    }

    /*
    #[test]
    fn test_scalar_ser(){
        test_scalar_serializarion()::<super::BNScalar>();
    }
    */
}


#[cfg(test)]
mod elgamal_over_bn_groups {
    use crate::basic_crypto::elgamal::elgamal_test;

    #[test]
    fn verification_g1(){
        elgamal_test::verification::<super::BNG1>();
    }

    #[test]
    fn decryption_g1(){
        elgamal_test::decryption::<super::BNG1>();
    }

    #[test]
    fn verification_g2(){
        elgamal_test::verification::<super::BNG1>();
    }

    #[test]
    fn decryption_g2(){
        elgamal_test::decryption::<super::BNG2>();
    }


    /*
    #[test]
    fn to_json(){
        elgamal_test::to_json::<super::BNG1>();
    }

    #[test]
    fn to_message_pack(){
        elgamal_test::to_message_pack::<super::BNG1>();
    }
    */
}

#[cfg(test)]
mod credentials_over_bn {

    #[test]
    fn single_attribute(){
        crate::credentials::credentials_tests::single_attribute::<super::BNGt>();
    }

    #[test]
    fn two_attributes(){
        crate::credentials::credentials_tests::two_attributes::<super::BNGt>();
    }
}