use crate::basics::elgamal::{ElGamalCiphertext, ElGamalEncKey};
use crate::sigma::{sigma_prove, sigma_verify_scalars, SigmaProof, SigmaTranscript};
use bulletproofs::PedersenGens;
use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::traits::{Identity, MultiscalarMul};
use merlin::Transcript;
use rand_core::{CryptoRng, RngCore};
use utils::errors::ZeiError;
use utils::serialization;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PedersenElGamalEqProof {
  #[serde(with = "serialization::zei_obj_serde")]
  z1: Scalar, // c*m + r_1
  #[serde(with = "serialization::zei_obj_serde")]
  z2: Scalar, // c*r + r_2
  #[serde(with = "serialization::zei_obj_serde")]
  e1: ElGamalCiphertext<RistrettoPoint>, // (r_2*G, r1*g + r2*PK)
  #[serde(with = "serialization::zei_obj_serde")]
  c1: RistrettoPoint, // r_1*g + r_2*H
}

// Initiate transcript for Pedersen-Elgamal aggregate proof
fn init_pedersen_elgamal_aggregate(transcript: &mut Transcript,
                                   pc_gens: &PedersenGens,
                                   public_key: &ElGamalEncKey<RistrettoPoint>,
                                   ctexts: &[ElGamalCiphertext<RistrettoPoint>],
                                   commitments: &[RistrettoPoint]) {
  let mut public_elems = vec![];
  public_elems.push(&pc_gens.B);
  public_elems.push(&pc_gens.B_blinding);
  public_elems.push(&public_key.0);
  for ctext in ctexts {
    public_elems.push(&ctext.e1);
    public_elems.push(&ctext.e2);
  }
  for commitment in commitments {
    public_elems.push(commitment);
  }
  transcript.init_sigma(b"PedersenElGamalAggEq", &[], public_elems.as_slice());
}

// Initiate transcript for PedersenElgamal proof and return proof elements,
// lhs indices matrix and rhs indices vector to be used as input to the sigma protocol
fn init_pok_pedersen_elgamal<'a>(transcript: &mut Transcript,
                                 identity: &'a RistrettoPoint,
                                 pc_gens: &'a PedersenGens,
                                 public_key: &'a ElGamalEncKey<RistrettoPoint>,
                                 ctext: &'a ElGamalCiphertext<RistrettoPoint>,
                                 commitment: &'a RistrettoPoint)
                                 -> (Vec<&'a RistrettoPoint>, Vec<Vec<usize>>, Vec<usize>) {
  transcript.append_message(b"new_domain", b"Dlog proof");
  let elems = vec![identity,
                   &pc_gens.B,
                   &pc_gens.B_blinding,
                   &public_key.0,
                   &ctext.e1,
                   &ctext.e2,
                   commitment];
  let lhs_matrix = vec![
    vec![0,1], // m*0 + r*B = ctext.e1
    vec![1,3], // m*B + r*PK = ctext.e2
    vec![1,2], // m*B + r*B_blinding = commitment
  ];
  let rhs_vec = vec![4, 5, 6]; // e1, e2, commitment
  (elems, lhs_matrix, rhs_vec)
}
// I compute a proof that ctext and commitment encrypts/holds m under same randomness r.
// assumes transcript already contains ciphertexts and commitments
pub fn pedersen_elgamal_eq_prove<R: CryptoRng + RngCore>(transcript: &mut Transcript,
                                                         prng: &mut R,
                                                         m: &Scalar,
                                                         r: &Scalar,
                                                         public_key: &ElGamalEncKey<RistrettoPoint>,
                                                         ctext: &ElGamalCiphertext<RistrettoPoint>,
                                                         commitment: &RistrettoPoint)
                                                         -> PedersenElGamalEqProof {
  let pc_gens = PedersenGens::default();
  let identity = RistrettoPoint::identity();
  let (elems, lhs_matrix, _) =
    init_pok_pedersen_elgamal(transcript, &identity, &pc_gens, public_key, ctext, commitment);
  let proof = sigma_prove(transcript,
                          prng,
                          elems.as_slice(),
                          lhs_matrix.as_slice(),
                          &[m, r]);
  PedersenElGamalEqProof { z1: proof.responses[0],
                           z2: proof.responses[1],
                           e1: ElGamalCiphertext { e1: proof.commitments[0],
                                                   e2: proof.commitments[1] },
                           c1: proof.commitments[2] }
}

fn pedersem_elgamal_eq_verify_scalars<R: CryptoRng + RngCore>(transcript: &mut Transcript,
                                                              prng: &mut R,
                                                              pc_gens: &PedersenGens,
                                                              public_key: &ElGamalEncKey<RistrettoPoint>,
                                                              ctext: &ElGamalCiphertext<RistrettoPoint>,
                                                              commitment: &RistrettoPoint,
                                                              proof: &PedersenElGamalEqProof)
                                                              -> Vec<Scalar> {
  let identity = RistrettoPoint::identity();
  let (elems, lhs_matrix, rhs_vec) =
    init_pok_pedersen_elgamal(transcript, &identity, &pc_gens, public_key, ctext, commitment);
  let sigma_proof = SigmaProof { commitments: vec![proof.e1.e1, proof.e1.e2, proof.c1],
                                 responses: vec![proof.z1, proof.z2] };
  let mut scalar_vec = sigma_verify_scalars(transcript,
                                            prng,
                                            &elems,
                                            &lhs_matrix,
                                            &rhs_vec,
                                            &sigma_proof);
  scalar_vec.remove(0);
  scalar_vec
}

// verify a pedersen/elgamal equality proof against ctext and commitment using aggregation
// technique and a single multiexponentiation check.
// assumes transcript already contains ciphertexts and commitments
#[allow(dead_code)]
fn pedersen_elgamal_eq_verify<R: CryptoRng + RngCore>(transcript: &mut Transcript,
                                                      prng: &mut R,
                                                      pc_gens: &PedersenGens,
                                                      public_key: &ElGamalEncKey<RistrettoPoint>,
                                                      ctext: &ElGamalCiphertext<RistrettoPoint>,
                                                      commitment: &RistrettoPoint,
                                                      proof: &PedersenElGamalEqProof)
                                                      -> Result<(), ZeiError> {
  let scalars = pedersem_elgamal_eq_verify_scalars(transcript, prng, pc_gens, public_key, ctext,
                                                   commitment, proof);

  let elems = [pc_gens.B,
               pc_gens.B_blinding,
               public_key.0,
               ctext.e1,
               ctext.e2,
               *commitment,
               proof.e1.e1,
               proof.e1.e2,
               proof.c1];
  let multi_exp = RistrettoPoint::multiscalar_mul(scalars, &elems);

  if multi_exp != RistrettoPoint::identity() {
    Err(ZeiError::ZKProofVerificationError)
  } else {
    Ok(())
  }
}

fn get_linear_combination_scalars(transcript: &mut Transcript, n: usize) -> Vec<Scalar> {
  if n == 0 {
    return vec![];
  }
  let mut r = vec![Scalar::one()];
  for _ in 0..n - 1 {
    r.push(transcript.get_challenge::<Scalar>());
  }
  r
}

/// Proof of Knowledge for PedersenElGamal equality proof, for a set of statement.
pub fn pedersen_elgamal_aggregate_eq_proof<R: CryptoRng + RngCore>(transcript: &mut Transcript,
                                                                   prng: &mut R,
                                                                   m: &[Scalar],
                                                                   r: &[Scalar],
                                                                   public_key: &ElGamalEncKey<RistrettoPoint>,
                                                                   ctexts: &[ElGamalCiphertext<RistrettoPoint>],
                                                                   commitments: &[RistrettoPoint])
                                                                   -> PedersenElGamalEqProof {
  let n = m.len();
  assert_eq!(n, m.len());
  assert_eq!(n, r.len());
  assert_eq!(n, ctexts.len());
  assert_eq!(n, commitments.len());

  let pc_gens = PedersenGens::default();
  init_pedersen_elgamal_aggregate(transcript, &pc_gens, public_key, ctexts, commitments);

  // 1. compute x vector
  let x = get_linear_combination_scalars(transcript, n);
  // 2. compute linear combination
  let mut lc_m = Scalar::zero();
  let mut lc_r = Scalar::zero();
  let mut lc_e1 = RistrettoPoint::identity();
  let mut lc_e2 = RistrettoPoint::identity();
  let mut lc_c = RistrettoPoint::identity();
  for (xi, mi, ri, ctext, com) in izip!(x.iter(),
                                        m.iter(),
                                        r.iter(),
                                        ctexts.iter(),
                                        commitments.iter())
  {
    lc_m += xi * mi;
    lc_r += xi * ri;
    lc_e1 += xi * ctext.e1;
    lc_e2 += xi * ctext.e2;
    lc_c += xi * com;
  }
  let lc_ctext = ElGamalCiphertext { e1: lc_e1,
                                     e2: lc_e2 };
  // 3. call proof
  pedersen_elgamal_eq_prove(transcript, prng, &lc_m, &lc_r, public_key, &lc_ctext, &lc_c)
}

pub struct PedersenElGamalProofInstance<'a> {
  pub public_key: &'a ElGamalEncKey<RistrettoPoint>,
  pub ctexts: Vec<ElGamalCiphertext<RistrettoPoint>>,
  pub commitments: Vec<RistrettoPoint>,
  pub proof: &'a PedersenElGamalEqProof,
}

/// Verify a batch of PedersenElGamal aggregate proof instances with a single multiexponentiation
/// of size 2 + n*7 elems. Each instance verification equation is scaled by a random factor.
/// Then, scaled equations are aggregated into a single equation of size 2 + n*7 elements.
pub fn pedersen_elgamal_batch_aggregate_eq_verify<'a, R: CryptoRng + RngCore>(
  transcript: &mut Transcript,
  prng: &mut R,
  pc_gens: &PedersenGens,
  instances: &[PedersenElGamalProofInstance<'a>])
  -> Result<(), ZeiError> {
  let m = instances.len();
  // 2 common elems: B, B_blinding
  // 7 elems per instance: public key,
  //                       ctext.e1, ctext.e2, commitment,
  //                       proof.ctext.e1, proof.ctext.e2, proof.commitment
  let mut all_scalars = Vec::with_capacity(2 + m * 7);
  let mut all_elems = Vec::with_capacity(2 + m * 7);
  all_scalars.push(Scalar::zero());
  all_scalars.push(Scalar::zero());
  all_elems.push(pc_gens.B);
  all_elems.push(pc_gens.B_blinding);
  for instance in instances {
    let n = instance.ctexts.len();
    assert_eq!(n, instance.commitments.len());
    let mut inst_transcript = transcript.clone();
    let alpha = Scalar::random(prng);
    init_pedersen_elgamal_aggregate(&mut inst_transcript,
                                    &pc_gens,
                                    instance.public_key,
                                    &instance.ctexts,
                                    &instance.commitments);
    // 1. compute x vector
    let x = get_linear_combination_scalars(&mut inst_transcript, n);
    // 2. compute linear combination
    let mut lc_e1 = RistrettoPoint::identity();
    let mut lc_e2 = RistrettoPoint::identity();
    let mut lc_c = RistrettoPoint::identity();
    for (xi, ei, ci) in izip!(x.iter(),
                              instance.ctexts.iter(),
                              instance.commitments.iter())
    {
      lc_e1 += xi * ei.e1;
      lc_e2 += xi * ei.e2;
      lc_c += xi * ci;
    }
    let lc_e = ElGamalCiphertext { e1: lc_e1,
                                   e2: lc_e2 };

    let instance_scalars = pedersem_elgamal_eq_verify_scalars(&mut inst_transcript,
                                                              prng,
                                                              pc_gens,
                                                              instance.public_key,
                                                              &lc_e,
                                                              &lc_c,
                                                              instance.proof);

    all_scalars[0] += alpha * instance_scalars[0];
    all_scalars[1] += alpha * instance_scalars[1];
    all_elems.push(instance.public_key.0);
    all_elems.push(lc_e1);
    all_elems.push(lc_e2);
    all_elems.push(lc_c);
    all_elems.push(instance.proof.e1.e1);
    all_elems.push(instance.proof.e1.e2);
    all_elems.push(instance.proof.c1);
    for scalar in instance_scalars[2..].iter() {
      all_scalars.push(alpha * scalar);
    }
  }

  let multi_exp = RistrettoPoint::multiscalar_mul(all_scalars, all_elems);
  if multi_exp != RistrettoPoint::identity() {
    return Err(ZeiError::ZKProofBatchVerificationError);
  }

  Ok(())
}
/// Verification of Proof of Knowledge for PedersenElGamal equality proof, for a set of statement.
pub fn pedersen_elgamal_aggregate_eq_verify<R: CryptoRng + RngCore>(transcript: &mut Transcript,
                                                                    prng: &mut R,
                                                                    pc_gens: &PedersenGens,
                                                                    public_key: &ElGamalEncKey<RistrettoPoint>,
                                                                    ctexts: &[ElGamalCiphertext<RistrettoPoint>],
                                                                    commitments: &[RistrettoPoint],
                                                                    proof: &PedersenElGamalEqProof)
                                                                    -> Result<(), ZeiError> {
  let instance = PedersenElGamalProofInstance { public_key,
                                                ctexts: ctexts.to_vec(),
                                                commitments: commitments.to_vec(),
                                                proof };

  pedersen_elgamal_batch_aggregate_eq_verify(transcript, prng, pc_gens, &[instance])
    .map_err(|_| ZeiError::ZKProofVerificationError)
}

#[cfg(test)]
mod test {
  use super::PedersenElGamalEqProof;
  use crate::basics::elgamal::{elgamal_encrypt, elgamal_key_gen};
  use bulletproofs::PedersenGens;
  use curve25519_dalek::ristretto::RistrettoPoint;
  use curve25519_dalek::scalar::Scalar;
  use merlin::Transcript;
  use rand_chacha::ChaChaRng;
  use rand_core::SeedableRng;
  use rmp_serde::Deserializer;
  use serde::de::Deserialize;
  use serde::ser::Serialize;
  use utils::errors::ZeiError;

  #[test]
  fn good_proof_verify() {
    let m = Scalar::from(10u8);
    let r = Scalar::from(7657u32);
    let mut prng = ChaChaRng::from_seed([0u8; 32]);
    let pc_gens = PedersenGens::default();

    let (_sk, pk) = elgamal_key_gen::<_, RistrettoPoint>(&mut prng, &pc_gens.B);

    let ctext = elgamal_encrypt(&pc_gens.B, &m, &r, &pk);
    let commitment = pc_gens.commit(m, r);

    let mut prover_transcript = Transcript::new(b"test");
    let mut verifier_transcript = Transcript::new(b"test");

    let proof = super::pedersen_elgamal_eq_prove(&mut prover_transcript,
                                                 &mut prng,
                                                 &m,
                                                 &r,
                                                 &pk,
                                                 &ctext,
                                                 &commitment);
    let verify = super::pedersen_elgamal_eq_verify(&mut verifier_transcript,
                                                   &mut prng,
                                                   &pc_gens,
                                                   &pk,
                                                   &ctext,
                                                   &commitment,
                                                   &proof);
    assert_eq!(true, verify.is_ok());
  }

  #[test]
  fn bad_proof_verify() {
    let m = Scalar::from(10u8);
    let m2 = Scalar::from(11u8);
    let r = Scalar::from(7657u32);
    let mut prng = ChaChaRng::from_seed([0u8; 32]);
    let pc_gens = PedersenGens::default();

    let (_sk, pk) = elgamal_key_gen::<_, RistrettoPoint>(&mut prng, &pc_gens.B);

    let ctext = elgamal_encrypt(&pc_gens.B, &m, &r, &pk);
    let commitment = pc_gens.commit(m2, r);

    let mut prover_transcript = Transcript::new(b"test");
    let mut verifier_transcript = Transcript::new(b"test");
    let proof = super::pedersen_elgamal_eq_prove(&mut prover_transcript,
                                                 &mut prng,
                                                 &m,
                                                 &r,
                                                 &pk,
                                                 &ctext,
                                                 &commitment);
    let verify = super::pedersen_elgamal_eq_verify(&mut verifier_transcript,
                                                   &mut prng,
                                                   &pc_gens,
                                                   &pk,
                                                   &ctext,
                                                   &commitment,
                                                   &proof);
    assert_eq!(true, verify.is_err());
    assert_eq!(ZeiError::ZKProofVerificationError, verify.err().unwrap());
  }

  #[test]
  fn proof_aggregate() {
    let m1 = Scalar::from(11u8);
    let r1 = Scalar::from(7657u32);
    let m2 = Scalar::from(12u8);
    let r2 = Scalar::from(7658u32);
    let m3 = Scalar::from(13u8);
    let r3 = Scalar::from(7659u32);
    let m4 = Scalar::from(14u8);
    let r4 = Scalar::from(7660u32);
    let mut prng = ChaChaRng::from_seed([0u8; 32]);
    let pc_gens = PedersenGens::default();

    let (_sk, pk) = elgamal_key_gen::<_, RistrettoPoint>(&mut prng, &pc_gens.B);

    let ctext1 = elgamal_encrypt(&pc_gens.B, &m1, &r1, &pk);
    let commitment1 = pc_gens.commit(m1, r1);
    let ctext2 = elgamal_encrypt(&pc_gens.B, &m2, &r2, &pk);
    let commitment2 = pc_gens.commit(m2, r2);
    let ctext3 = elgamal_encrypt(&pc_gens.B, &m3, &r3, &pk);
    let commitment3 = pc_gens.commit(m3, r3);
    let ctext4 = elgamal_encrypt(&pc_gens.B, &m4, &r4, &pk);
    let commitment4 = pc_gens.commit(m4, r4);

    let ctexts = [ctext1, ctext2, ctext3, ctext4];
    let commitments = [commitment1, commitment2, commitment3, commitment4];
    let mut prover_transcript = Transcript::new(b"test");
    let mut verifier_transcript = Transcript::new(b"test");

    let proof = super::pedersen_elgamal_aggregate_eq_proof(&mut prover_transcript,
                                                           &mut prng,
                                                           &[m1, m2, m3, m4],
                                                           &[r1, r2, r3, r4],
                                                           &pk,
                                                           &ctexts,
                                                           &commitments);
    let verify = super::pedersen_elgamal_aggregate_eq_verify(&mut verifier_transcript,
                                                             &mut prng,
                                                             &pc_gens,
                                                             &pk,
                                                             &ctexts,
                                                             &commitments,
                                                             &proof);
    assert_eq!(verify, Ok(()));

    let mut prover_transcript = Transcript::new(b"test");
    let mut verifier_transcript = Transcript::new(b"test");
    let proof = super::pedersen_elgamal_aggregate_eq_proof(&mut prover_transcript,
                                                           &mut prng,
                                                           &[m1],
                                                           &[r1],
                                                           &pk,
                                                           &ctexts[..1],
                                                           &commitments[..1]);
    let verify = super::pedersen_elgamal_aggregate_eq_verify(&mut verifier_transcript,
                                                             &mut prng,
                                                             &pc_gens,
                                                             &pk,
                                                             &ctexts[..1],
                                                             &commitments[..1],
                                                             &proof);
    assert_eq!(verify, Ok(()));

    let mut prover_transcript = Transcript::new(b"test");
    let mut verifier_transcript = Transcript::new(b"test");
    let proof = super::pedersen_elgamal_aggregate_eq_proof(&mut prover_transcript,
                                                           &mut prng,
                                                           &[m2],
                                                           &[r2],
                                                           &pk,
                                                           &ctexts[1..2],
                                                           &commitments[1..2]);
    let verify = super::pedersen_elgamal_aggregate_eq_verify(&mut verifier_transcript,
                                                             &mut prng,
                                                             &pc_gens,
                                                             &pk,
                                                             &ctexts[1..2],
                                                             &commitments[1..2],
                                                             &proof);
    assert_eq!(verify, Ok(()));

    let mut prover_transcript = Transcript::new(b"test");
    let mut verifier_transcript = Transcript::new(b"test");
    let proof = super::pedersen_elgamal_aggregate_eq_proof(&mut prover_transcript,
                                                           &mut prng,
                                                           &[m2, m3],
                                                           &[r2, r3],
                                                           &pk,
                                                           &ctexts[1..3],
                                                           &commitments[1..3]);
    let verify = super::pedersen_elgamal_aggregate_eq_verify(&mut verifier_transcript,
                                                             &mut prng,
                                                             &pc_gens,
                                                             &pk,
                                                             &ctexts[1..3],
                                                             &commitments[1..3],
                                                             &proof);
    assert!(verify.is_ok());

    let mut prover_transcript = Transcript::new(b"test");
    let mut verifier_transcript = Transcript::new(b"test");
    let proof = super::pedersen_elgamal_aggregate_eq_proof(&mut prover_transcript,
                                                           &mut prng,
                                                           &[m1, m2, m3, m3],
                                                           &[r1, r2, r3, r4],
                                                           &pk,
                                                           &ctexts,
                                                           &commitments);
    let verify = super::pedersen_elgamal_aggregate_eq_verify(&mut verifier_transcript,
                                                             &mut prng,
                                                             &pc_gens,
                                                             &pk,
                                                             &ctexts,
                                                             &commitments,
                                                             &proof);
    assert!(verify.is_err());
    assert_eq!(ZeiError::ZKProofVerificationError, verify.err().unwrap());

    let mut prover_transcript = Transcript::new(b"test");
    let mut verifier_transcript = Transcript::new(b"test");
    let proof = super::pedersen_elgamal_aggregate_eq_proof(&mut prover_transcript,
                                                           &mut prng,
                                                           &[m1, m2, m3, m4],
                                                           &[r1, r2, r3, r1],
                                                           &pk,
                                                           &ctexts,
                                                           &commitments);
    let verify = super::pedersen_elgamal_aggregate_eq_verify(&mut verifier_transcript,
                                                             &mut prng,
                                                             &pc_gens,
                                                             &pk,
                                                             &ctexts,
                                                             &commitments,
                                                             &proof);
    assert!(verify.is_err());
    assert_eq!(ZeiError::ZKProofVerificationError, verify.err().unwrap());

    let mut prover_transcript = Transcript::new(b"test");
    let mut verifier_transcript = Transcript::new(b"test");
    let proof = super::pedersen_elgamal_aggregate_eq_proof(&mut prover_transcript,
                                                           &mut prng,
                                                           &[m1, m2, m3, m4],
                                                           &[r2, r2, r3, r4],
                                                           &pk,
                                                           &ctexts,
                                                           &commitments);

    let verify = super::pedersen_elgamal_aggregate_eq_verify(&mut verifier_transcript,
                                                             &mut prng,
                                                             &pc_gens,
                                                             &pk,
                                                             &ctexts,
                                                             &commitments,
                                                             &proof);
    assert!(verify.is_err());
    assert_eq!(ZeiError::ZKProofVerificationError, verify.err().unwrap());
  }

  #[test]
  fn to_json() {
    let m = Scalar::from(10u8);
    let r = Scalar::from(7657u32);
    let mut prng = ChaChaRng::from_seed([0u8; 32]);
    let pc_gens = PedersenGens::default();

    let (_sk, pk) = elgamal_key_gen::<_, RistrettoPoint>(&mut prng, &pc_gens.B);
    let ctext = elgamal_encrypt(&pc_gens.B, &m, &r, &pk);
    let commitment = pc_gens.commit(m, r);
    let mut transcript = Transcript::new(b"test");
    let proof = super::pedersen_elgamal_aggregate_eq_proof(&mut transcript,
                                                           &mut prng,
                                                           &[m],
                                                           &[r],
                                                           &pk,
                                                           &[ctext],
                                                           &[commitment]);

    let json_str = serde_json::to_string(&proof).unwrap();
    let proof_de = serde_json::from_str(&json_str).unwrap();
    assert_eq!(proof, proof_de, "Deserialized proof does not match");
  }

  #[test]
  fn to_message_pack() {
    let m = Scalar::from(10u8);
    let r = Scalar::from(7657u32);
    let mut prng = ChaChaRng::from_seed([0u8; 32]);
    let pc_gens = PedersenGens::default();

    let (_sk, pk) = elgamal_key_gen::<_, RistrettoPoint>(&mut prng, &pc_gens.B);

    let ctext = elgamal_encrypt(&pc_gens.B, &m, &r, &pk);
    let commitment = pc_gens.commit(m, r);
    let mut transcript = Transcript::new(b"test");
    let proof = super::pedersen_elgamal_aggregate_eq_proof(&mut transcript,
                                                           &mut prng,
                                                           &[m],
                                                           &[r],
                                                           &pk,
                                                           &[ctext],
                                                           &[commitment]);

    let mut vec = vec![];
    proof.serialize(&mut rmp_serde::Serializer::new(&mut vec))
         .unwrap();

    let mut de = Deserializer::new(&vec[..]);
    let proof_de: PedersenElGamalEqProof = Deserialize::deserialize(&mut de).unwrap();
    assert_eq!(proof, proof_de);
  }
}