use bulletproofs::{BulletproofGens, RangeProof, PedersenGens};
use crate::encryption::ZeiRistrettoCipher;
use crate::encryption::ZeiRistrettoCipherString;
use crate::errors::Error as ZeiError;
use crate::proofs::chaum_perdersen::{ChaumPedersenCommitmentEqProof, ChaumPedersenCommitmentEqProofString, chaum_pedersen_prove_eq, chaum_pedersen_verify_eq};
use crate::serialization::{RangeProofString, CompressedRistrettoString,
                           ScalarString, PublicKeyString};
use crate::setup::PublicParams;
use crate::utils::u64_to_bigendian_u8array;
use crate::utils::u8_bigendian_slice_to_u64;
use crate::setup::Balance;
use crate::setup::BULLET_PROOF_RANGE;
use curve25519_dalek::ristretto::{ CompressedRistretto, RistrettoPoint };
use curve25519_dalek::scalar::Scalar;
use merlin::Transcript;
use rand::CryptoRng;
use rand::Rng;
use schnorr::PublicKey;
use schnorr::SecretKey;
use std::convert::TryFrom;

#[derive(Serialize, Deserialize, Debug)]
pub struct Transaction {
    /*
     * I represent a transaction. I contain
     * - a range proof (0, val_max) for the senders updated balance and transaction amount,
     * - a Pedersen commitment for the transfer,
     * - and a encrypted box for the receiver that includes the transfered amount and the blinding
     * factor of the transaction commitment.
     * - boolean indicating whether transaction is confidential for asset type or not
     * - A proof of equality of asset type
     * - The sender and the receiver asset commitments
     */
    pub transaction_range_proof: bulletproofs::RangeProof,
    pub transaction_commitment: CompressedRistretto,
    pub lockbox: ZeiRistrettoCipher,
    pub do_confidential_asset: bool,
    pub asset_eq_proof: ChaumPedersenCommitmentEqProof,
    pub sender_asset_commitment: CompressedRistretto,
    pub receiver_asset_commitment: CompressedRistretto,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TxParams{
    /*
     * I am helper structure to send/receive the data for a transaction
     *
     */
    pub receiver_pk: PublicKey,
    pub receiver_asset_commitment: CompressedRistretto,
    pub receiver_asset_opening: Scalar,
    pub transfer_amount: Balance,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TxParamsString{
    receiver_pk: PublicKeyString,
    receiver_asset_commitment: CompressedRistrettoString,
    receiver_asset_opening: ScalarString,
    transfer_amount: u64,
}

impl TryFrom<TxParamsString> for TxParams{
    type Error = ZeiError;
    fn try_from(tx: TxParamsString) -> Result<TxParams, ZeiError> {
        Ok(
            TxParams{
                receiver_pk: PublicKey::try_from(tx.receiver_pk)?,
                receiver_asset_commitment: CompressedRistretto::try_from(&tx.receiver_asset_commitment)?,
                receiver_asset_opening: Scalar::try_from(&tx.receiver_asset_opening)?,
                transfer_amount: tx.transfer_amount,
            }
        )
    }
}

impl From<TxParams> for TxParamsString{
    fn from(tx: TxParams) -> TxParamsString {
        TxParamsString{
            receiver_pk: PublicKeyString::from(tx.receiver_pk),
            receiver_asset_commitment: CompressedRistrettoString::from(&tx.receiver_asset_commitment),
            receiver_asset_opening: ScalarString::from(&tx.receiver_asset_opening),
            transfer_amount: tx.transfer_amount,
        }
    }
}


impl Transaction {
    pub fn new<R>(mut csprng: &mut R,
                  tx_params: &TxParams,
                  account_balance: Balance,
                  account_blind: &Scalar,
                  asset: &Scalar,
                  sender_asset_opening: &Scalar,
                  sender_asset_commitment: &CompressedRistretto,
                  do_confidential_asset: bool) -> Result<(Transaction, Scalar), ZeiError>
    where R: CryptoRng + Rng,
    {
        /*
         * I create a new transaction. 
         * - Create new public parameters
         * - Sample Fresh blinding factor [blind], its a scalar.
         * - Create Commitment ->  g^amount * h^[blind] == CommT
         * - Create rangeproof for amount & use [blind] as randomness == RP_T
         * - Create Commitment ->  g^(Balance - amount) * h^(Opening - blind) == CommS
         * - Create rangeproof for (Balance - transfer_amount) & use Opening - blind as randomness == RP_S
         * - Encrypt transfered amount and blinding factor to receiver
         * - Create and return the transaction
         */

        let mut params = PublicParams::new();
        let blinding_t = Scalar::random(csprng);
        let tx_amount = tx_params.transfer_amount;
        let sender_updated_balance = account_balance - tx_amount;
        let sender_updated_account_blind = account_blind - blinding_t;

        let range_proof_result = RangeProof::prove_multiple(
            &params.bp_gens,
            &params.pc_gens,
            &mut params.transcript,
            &[u64::from(tx_amount), u64::from(sender_updated_balance)],
            &[blinding_t, sender_updated_account_blind],
            BULLET_PROOF_RANGE);

        let (proof_agg, commitments_agg) = match range_proof_result {
            Ok((pf_agg, comm_agg)) => (pf_agg, comm_agg),
            Err(_) => { return Err(ZeiError::TxProofError);},
        };
        let mut asset_eq_proof = ChaumPedersenCommitmentEqProof::default();

        if do_confidential_asset {
            asset_eq_proof = chaum_pedersen_prove_eq(&mut csprng,
                                                            &params.pc_gens,
                                                            asset,
                                                            sender_asset_commitment,
                                                            &tx_params.receiver_asset_commitment,
                                                            &sender_asset_opening,
                                                            &tx_params.receiver_asset_opening);
        }

        let mut to_encrypt = Vec::new();
        to_encrypt.extend_from_slice(&u64_to_bigendian_u8array(tx_amount));
        to_encrypt.extend_from_slice(&blinding_t.to_bytes());
        let receiver_public_key = &tx_params.receiver_pk.get_curve_point()?.compress();
        let lbox = ZeiRistrettoCipher::encrypt(csprng, receiver_public_key, &to_encrypt)?;

        let tx = Transaction {
            transaction_range_proof: proof_agg,
            transaction_commitment: commitments_agg[0],
            lockbox: lbox,
            do_confidential_asset,
            asset_eq_proof,
            sender_asset_commitment: sender_asset_commitment.clone(),
            receiver_asset_commitment: tx_params.receiver_asset_commitment,
        };

       Ok((tx, blinding_t))
    }

    pub fn recover_plaintext(&self, sk: &SecretKey) -> (Balance, Scalar) {
        /*
         * I recover the sent amount and blind factor from the encryted box in a transaction
         *
         */
        //secret key to scalar:
        //decode secret key into scalar
        //TODO have SecretKey = Scalar
        let mut secret_key_bytes = sk.to_bytes();
        secret_key_bytes[0]  &= 248;
        secret_key_bytes[31] &= 127;
        secret_key_bytes[31] |= 64;

        let secret_key = Scalar::from_bits(secret_key_bytes);
        let unlocked = self.lockbox.decrypt(&secret_key).unwrap();
        let (raw_amount, raw_blind) = unlocked.split_at(8);

        //TODO: the following assume amounts are 64 bits
        let p_amount = u8_bigendian_slice_to_u64(&raw_amount);

        let mut bytes: [u8;32] = Default::default();
        bytes.copy_from_slice(&raw_blind[0..32]);
        let blind_scalar = Scalar::from_bits(bytes);

        (p_amount, blind_scalar)
    }

}


pub fn validator_verify(tx: &Transaction,
                        sender_prev_com: &CompressedRistretto,
                        sender_asset: &CompressedRistretto,
                        receiver_asset: &CompressedRistretto) -> Result<bool, ZeiError> {
    /*
     * Run by validator. I verify the transaction:
     * a) sender new balance commitment must match commitmment in transaction
     * b) Verify range proofs
     * c) Verify same asset type
     * If tx.do_confidential_asset, then sender_asset and receiver_asset are commitments, otherwise
     * they are simple digests of the the asset structure
     */

    let mut transcript = Transcript::new(b"Zei Range Proof");
    let pc_gens = PedersenGens::default();
    //TODO:This probably shouldn't be regenerated every time
    let bp_gens = BulletproofGens::new(BULLET_PROOF_RANGE, 2);

    let tx_comm = tx.transaction_commitment.decompress()?;
    let derived_sender_comm = (sender_prev_com.decompress()? - tx_comm).compress();

    let verify_t = RangeProof::verify_multiple(
        &tx.transaction_range_proof,
        &bp_gens,
        &pc_gens,
        &mut transcript,
        &[tx.transaction_commitment, derived_sender_comm],
        BULLET_PROOF_RANGE,
    );

    let mut vrfy_ok = verify_t.is_ok();
    if vrfy_ok {
        if tx.do_confidential_asset {
            vrfy_ok = chaum_pedersen_verify_eq(
                &pc_gens,
                &sender_asset,
                &receiver_asset,
                &tx.asset_eq_proof)?;
        }
        else{
            vrfy_ok = sender_asset == receiver_asset;
        }
    }
    Ok(vrfy_ok)
}


pub fn receiver_verify(tx_amount: u32, tx_blind: Scalar, new_commit: RistrettoPoint, recv_old_commit: RistrettoPoint) -> bool {
    /*
     * Run by receiver: I verify the new commitment to my balance using the new blinding factor
     * and old balance commitment.
     *
     */
    let pc_gens = PedersenGens::default();
    let compute_new_commit = pc_gens.commit(Scalar::from(tx_amount), tx_blind);
    let updated_commitment = compute_new_commit + recv_old_commit;
    new_commit == updated_commitment
}


//serialization of transaction strucures
#[derive(Serialize, Deserialize, Debug)]
pub struct TransactionString{
    transaction_range_proof: RangeProofString,
    transaction_commitment: CompressedRistrettoString,
    lockbox: ZeiRistrettoCipherString,
    do_confidential_asset: bool,
    asset_eq_proof: ChaumPedersenCommitmentEqProofString,
    sender_asset_commitment: CompressedRistrettoString,
    receiver_asset_commitment: CompressedRistrettoString,
}

impl TryFrom<&Transaction> for TransactionString {
    type Error = ZeiError;
    fn try_from(a: &Transaction) -> Result<TransactionString, ZeiError>{
        Ok(TransactionString{
            transaction_range_proof: RangeProofString::from(&a.transaction_range_proof),
            transaction_commitment: CompressedRistrettoString::from(&a.transaction_commitment),
            lockbox: ZeiRistrettoCipherString::try_from(&a.lockbox)?,
            do_confidential_asset: a.do_confidential_asset,
            asset_eq_proof: ChaumPedersenCommitmentEqProofString::from(&a.asset_eq_proof),
            sender_asset_commitment: CompressedRistrettoString::from(&a.sender_asset_commitment),
            receiver_asset_commitment: CompressedRistrettoString::from(&a.receiver_asset_commitment),
        })
    }
}

impl TryFrom<&TransactionString> for Transaction{
    type Error = ZeiError;
    fn try_from(a: &TransactionString) -> Result<Transaction, ZeiError> {
        let transaction_range_proof = RangeProof::try_from(&a.transaction_range_proof)?;
        let transaction_commitment = CompressedRistretto::try_from(&a.transaction_commitment)?;
        let lockbox: ZeiRistrettoCipher = ZeiRistrettoCipher::try_from(&a.lockbox)?;
        let do_confidential_asset = a.do_confidential_asset;
        let asset_eq_proof = ChaumPedersenCommitmentEqProof::try_from(&a.asset_eq_proof)?;
        let sender_asset_commitment = CompressedRistretto::try_from(&a.sender_asset_commitment)?;
        let receiver_asset_commitment = CompressedRistretto::try_from(&a.receiver_asset_commitment)?;
        Ok(Transaction{
            transaction_range_proof,
            transaction_commitment,
            lockbox,
            do_confidential_asset,
            asset_eq_proof,
            sender_asset_commitment,
            receiver_asset_commitment,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::account::Account;
    use curve25519_dalek::scalar::Scalar;
    use bulletproofs::{BulletproofGens, PedersenGens, RangeProof};
    use merlin::Transcript;
    use rand_chacha::ChaChaRng;
    use rand::SeedableRng;
    use crate::account::AssetBalance;

    #[test]
    fn test_new_transaction() {
        let asset_id = "default currency";
        let mut csprng: ChaChaRng;
        csprng  = ChaChaRng::from_seed([0u8; 32]);

        //def pederson from lib with Common Reference String
        let pc_gens = PedersenGens::default();

        //Account A
        let mut acc_a = Account::new(&mut csprng);
        acc_a.add_asset(&mut csprng, asset_id, false, 8000000000);
        //Account B
        let mut acc_b = Account::new(&mut csprng);
        acc_b.add_asset(&mut csprng, asset_id, false, 50);

        let new_tx = TxParams {
            receiver_pk: acc_b.keys.public,
            transfer_amount: 5000000000,
            receiver_asset_opening: Scalar::from(0u8),
            receiver_asset_commitment:acc_b.get_asset_balance(asset_id).asset_commitment,
        };

        //
        //Create Proofs
        //

        let mut transcript = Transcript::new(b"Zei Range Proof");
        let bp_gens = BulletproofGens::new(BULLET_PROOF_RANGE, 2);

        let blinding_t = Scalar::random(&mut csprng);

        let values = &[new_tx.transfer_amount, acc_a.get_balance(asset_id) - 100u64];
        let blindings = &[blinding_t, acc_a.get_asset_balance(asset_id).balance_blinding - blinding_t];

        let (_, commitments_agg) = RangeProof::prove_multiple(
            &bp_gens,
            &pc_gens,
            &mut transcript,
            values,
            blindings,
            BULLET_PROOF_RANGE,
            ).expect("A real program could handle errors");

        let tx_derived_commit = pc_gens.commit(Scalar::from(new_tx.transfer_amount), blinding_t);

        assert_eq!(tx_derived_commit, commitments_agg[0].decompress().unwrap());
    }

    #[test]
    fn test_confidential_asset_transaction(){
        let asset_id = "default_currency";
        let transfer_amount = 0x01234567;
        let mut csprng: ChaChaRng;
        csprng  = ChaChaRng::from_seed([0u8; 32]);

        // source account setup
        let mut acc_src = Account::new(&mut csprng);
        acc_src.add_asset(&mut csprng, asset_id, true, 0x12345678);
        let src_asset_balance = acc_src.get_asset_balance(asset_id);


        // destination account setup
        let mut acc_dst = Account::new(&mut csprng);
        acc_dst.add_asset(&mut csprng, asset_id, true, 50);
        let dst_pk = acc_dst.get_public_key();
        let dst_asset_balance = acc_dst.get_asset_balance(asset_id);

        let asset_scalar = dst_asset_balance.asset_info.compute_scalar_hash();

        do_transaction_validation(src_asset_balance, dst_asset_balance,&asset_scalar,dst_pk, transfer_amount, true, true);

        // accounts asset do not match
        acc_dst.add_asset(&mut csprng, "other asset", true, 50);

        let dst_asset_balance = acc_dst.get_asset_balance("other asset");
        do_transaction_validation(src_asset_balance, dst_asset_balance,&asset_scalar, dst_pk, transfer_amount, true,false);

    }

    #[test]
    fn test_non_confidential_asset_transaction(){
        let asset_id = "default_currency";
        let transfer_amount = 0x01234567;
        let mut csprng: ChaChaRng;
        csprng  = ChaChaRng::from_seed([0u8; 32]);


        let mut acc_src = Account::new(&mut csprng);
        acc_src.add_asset(&mut csprng, asset_id, false, 0x12345678);
        let src_asset_balance = acc_src.get_asset_balance(asset_id);

        // destination account setup
        let mut acc_dst = Account::new(&mut csprng);
        acc_dst.add_asset(&mut csprng, asset_id, false, 50);
        let dst_pk = acc_dst.get_public_key();
        let dst_asset_balance = acc_dst.get_asset_balance(asset_id);

        let asset_scalar = dst_asset_balance.asset_info.compute_scalar_hash();

        do_transaction_validation(src_asset_balance, dst_asset_balance, &asset_scalar, dst_pk,transfer_amount, false, true);
    }

    fn do_transaction_validation(src_asset_balance: &AssetBalance,
                                 dst_asset_balance: &AssetBalance,
                                 asset: &Scalar,
                                 dst_pk: PublicKey,
                                 transfer_amount: u64,
                                 confidential_asset: bool,
                                 expected: bool){

        let mut csprng: ChaChaRng;
        csprng  = ChaChaRng::from_seed([0u8; 32]);

        let new_tx = TxParams {
            receiver_pk: dst_pk,
            transfer_amount,
            receiver_asset_opening: dst_asset_balance.asset_blinding,
            receiver_asset_commitment: dst_asset_balance.asset_commitment,
        };

        let (tx,_)  = Transaction::new(&mut csprng,
                                       &new_tx,
                                       src_asset_balance.balance,
                                       &src_asset_balance.balance_blinding,
                                        asset,
                                       &src_asset_balance.asset_blinding,
                                       &src_asset_balance.asset_commitment,
                                       confidential_asset).unwrap();

        let vrfy_ok = validator_verify(&tx,
                                       &src_asset_balance.balance_commitment,
                                       &src_asset_balance.asset_commitment,
                                       &dst_asset_balance.asset_commitment).unwrap();
        assert_eq!(expected, vrfy_ok);

    }

    #[test]
    fn test_transaction_serialization(){
        let asset_id = "default_currency";
        let transfer_amount = 0x01000000;
        let mut csprng: ChaChaRng;
        csprng  = ChaChaRng::from_seed([0u8; 32]);

        // source account setup
        let mut acc_src = Account::new(&mut csprng);
        acc_src.add_asset(&mut csprng, asset_id, true, 0x02000000);

        // destination account stup
        let mut acc_dst = Account::new(&mut csprng);
        acc_dst.add_asset(&mut csprng, asset_id, true, 0);

        let asset_scalar = acc_dst.balances[asset_id].asset_info.compute_scalar_hash();

        let new_tx = TxParams {
            receiver_pk: acc_dst.keys.public,
            transfer_amount: transfer_amount,
            receiver_asset_opening: acc_dst.balances[asset_id].asset_blinding,
            receiver_asset_commitment: acc_dst.balances[asset_id].asset_commitment,
        };

        let (tx,_)  = Transaction::new(&mut csprng,
                                       &new_tx,
                                       acc_src.balances[asset_id].balance,
                                       &acc_src.balances[asset_id].balance_blinding,
                                       &asset_scalar,
                                       &acc_src.balances[asset_id].asset_blinding,
                                       &acc_src.balances[asset_id].asset_commitment,
                                       true).unwrap();

        let tx_json = serde_json::to_string(&TransactionString::try_from(&tx).unwrap()).unwrap();
        let deserialized_tx = Transaction::try_from(
            &serde_json::from_str::<TransactionString>(&tx_json).unwrap());

        let dtx = deserialized_tx.unwrap();

        assert_eq!(tx.transaction_commitment, dtx.transaction_commitment);
        assert_eq!(tx.receiver_asset_commitment, dtx.receiver_asset_commitment);
        assert_eq!(tx.sender_asset_commitment, dtx.sender_asset_commitment);
        assert_eq!(tx.do_confidential_asset, dtx.do_confidential_asset);
        assert_eq!(tx.asset_eq_proof, dtx.asset_eq_proof);
        assert_eq!(tx.lockbox, dtx.lockbox);
    }
}
