use crate::algebra::bls12_381::{BLSGt, BLSScalar, BLSG1};
use crate::api::anon_creds::{ACIssuerPublicKey, ACRevealSig};
use crate::errors::ZeiError;
use crate::utils::byte_slice_to_scalar;
use rand::{CryptoRng, Rng};

pub type ElGamalPublicKey = crate::basic_crypto::elgamal::ElGamalPublicKey<BLSG1>;
pub type ElGamalCiphertext = crate::basic_crypto::elgamal::ElGamalCiphertext<BLSG1>;
pub type ElGamalSecretKey = crate::basic_crypto::elgamal::ElGamalSecretKey<BLSScalar>;

pub type ConfidentialAC = crate::crypto::conf_cred_reveal::ConfidentialAC<BLSGt>;

/// Produced a CACProof for a single instance of a confidential anonymous reveal. Proof asserts
/// that a list of attributes can be decrypted from a list of ciphertexts under recv_enc_pub_key,
/// and that these attributed verify an anonymous credential reveal proof.
/// * `prng` - randomness source
/// * `cred_issuer_pk` - (signing) public key of the credential issuer
/// * `enc_key` - encryption public key of the receiver
/// * `attrs` - attributes to prove knowledge of
/// * `reveal_map` - indicates position of each attribute to prove
/// * `ac_reveal_sig` - proof that the issuer has signed some attributes
/// * `returns` - proof that the ciphertexts contains the attributes that have been signed by some issuer for the user.
/// # Example
/// ```
/// use zei::api::anon_creds::{ac_keygen_issuer, ac_keygen_user, ac_sign, ac_reveal};
/// use rand_chacha::ChaChaRng;
/// use rand::SeedableRng;
/// use zei::api::conf_cred_reveal::{cac_create, cac_verify};
/// use zei::basic_crypto::elgamal::elgamal_keygen;
/// use zei::algebra::bls12_381::{BLSScalar, BLSG1};
/// use zei::algebra::groups::Group;
/// let mut prng = ChaChaRng::from_seed([0u8;32]);
/// let (issuer_pk, issuer_sk) = ac_keygen_issuer(&mut prng, 3);
/// let (user_pk, user_sk) = ac_keygen_user(&mut prng, &issuer_pk);
/// let (_, enc_key) = elgamal_keygen::<_, BLSScalar, BLSG1>(&mut prng, &BLSG1::get_base());
/// let attr1 = b"attr1";
/// let attr2 = b"attr2";
/// let attr3 = b"attr3";
/// let attrs = [attr1.as_ref(), attr2.as_ref(), attr3.as_ref()];
/// let bitmap = [false, true, false];
/// let ac_sig = ac_sign(&mut prng, &issuer_sk, &user_pk, &attrs[..]);
/// let credential = ac_reveal(&mut prng, &user_sk, &issuer_pk, &ac_sig, &attrs[..], &bitmap[..]).unwrap();
/// let conf_reveal_proof = cac_create(&mut prng, &issuer_pk, &enc_key, &attrs[..], &bitmap[..], &credential).unwrap();
/// assert!(cac_verify(&issuer_pk, &enc_key, &bitmap[..], &conf_reveal_proof).is_ok())
/// ```
pub fn cac_create<R: CryptoRng + Rng>(prng: &mut R,
                                      cred_issuer_pk: &ACIssuerPublicKey,
                                      enc_key: &ElGamalPublicKey,
                                      attrs: &[&[u8]],
                                      reveal_map: &[bool],
                                      ac_reveal_sig: &ACRevealSig)
                                      -> Result<ConfidentialAC, ZeiError> {
  let attrs_scalar: Vec<BLSScalar> = attrs.iter()
                                          .map(|x| byte_slice_to_scalar::<BLSScalar>(*x))
                                          .collect();
  crate::crypto::conf_cred_reveal::cac_create(prng,
                                              cred_issuer_pk,
                                              enc_key,
                                              attrs_scalar.as_slice(),
                                              reveal_map,
                                              ac_reveal_sig)
}

/// Verifies a Confidential Anonymous Credential reveal proof. Proof asserts
/// that a list of ciphertexts encodes attributes under `enc_key` such that
/// these verify an anonymous credential reveal proof.
/// * `prng` - randomness source
/// * `issuer_pk` - (signing) public key of the credential issuer
/// * `enc_key` - encryption public key of the receiver
/// * `reveal_map` - indicates position of each attribute to prove
/// * `cac` - List of ciphertext and the corresponding proof
/// # Example
/// ```
///  // see zei::api::conf_cred_reveal::cac_create;
/// ```
pub fn cac_verify(issuer_pk: &ACIssuerPublicKey,
                  enc_key: &ElGamalPublicKey,
                  reveal_map: &[bool],
                  cac: &ConfidentialAC)
                  -> Result<(), ZeiError> {
  crate::crypto::conf_cred_reveal::cac_verify(issuer_pk, enc_key, reveal_map, cac)
}
