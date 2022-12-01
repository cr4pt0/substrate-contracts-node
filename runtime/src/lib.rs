#![cfg_attr(not(feature = "std"), no_std)]
// `construct_runtime!` does a lot of recursion and requires us to increase the limit to 256.
#![recursion_limit = "256"]

// Make the WASM binary available.
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

use frame_support::{traits::OnRuntimeUpgrade, weights::DispatchClass};
use frame_system::limits::{BlockLength, BlockWeights};
use frame_system::EnsureRoot;
use pallet_contracts::{migration, DefaultContractAccessWeight};
use sp_api::impl_runtime_apis;
use sp_core::{crypto::KeyTypeId, OpaqueMetadata};
use sp_runtime::{
	MultiAddress,
	create_runtime_str, generic, impl_opaque_keys,
	traits::{AccountIdLookup, BlakeTwo256, Block as BlockT, IdentifyAccount, Verify},
	transaction_validity::{TransactionSource, TransactionValidity},
	ApplyExtrinsicResult, MultiSignature,
};
use sp_std::prelude::*;
#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;

// A few exports that help ease life for downstream crates.
pub use frame_support::{
	construct_runtime, parameter_types,
	traits::{ConstU128, ConstU32, ConstU8, KeyOwnerProofSystem, Randomness, StorageInfo},
	weights::{
		constants::{BlockExecutionWeight, ExtrinsicBaseWeight, RocksDbWeight, WEIGHT_PER_SECOND},
		IdentityFee, Weight,
	},
	StorageValue,
};
pub use frame_system::Call as SystemCall;
pub use pallet_balances::Call as BalancesCall;
pub use pallet_timestamp::Call as TimestampCall;
use pallet_transaction_payment::CurrencyAdapter;
#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;
pub use sp_runtime::{Perbill, Permill};

/// An index to a block.
pub type BlockNumber = u32;

/// Alias to 512-bit hash when used in the context of a transaction signature on the chain.
pub type Signature = MultiSignature;

/// Some way of identifying an account on the chain. We intentionally make it equivalent
/// to the public key of our transaction signing scheme.
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

/// Balance of an account.
pub type Balance = u128;

/// Index of a transaction in the chain.
pub type Index = u32;

/// A hash of some data used by the chain.
pub type Hash = sp_core::H256;

/// Opaque types. These are used by the CLI to instantiate machinery that don't need to know
/// the specifics of the runtime. They can then be made to be agnostic over specific formats
/// of data like extrinsics, allowing for them to continue syncing the network through upgrades
/// to even the core data structures.
pub mod opaque {
	use super::*;

	pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;

	/// Opaque block header type.
	pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
	/// Opaque block type.
	pub type Block = generic::Block<Header, UncheckedExtrinsic>;
	/// Opaque block identifier type.
	pub type BlockId = generic::BlockId<Block>;

	impl_opaque_keys! {
		pub struct SessionKeys {}
	}
}
// To learn more about runtime versioning, see:
// https://docs.substrate.io/main-docs/build/upgrade#runtime-versioning
#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
	spec_name: create_runtime_str!("substrate-contracts-node"),
	impl_name: create_runtime_str!("substrate-contracts-node"),
	authoring_version: 1,
	// The version of the runtime specification. A full node will not attempt to use its native
	//   runtime in substitute for the on-chain Wasm runtime unless all of `spec_name`,
	//   `spec_version`, and `authoring_version` are the same between Wasm and native.
	// This value is set to 100 to notify Polkadot-JS App (https://polkadot.js.org/apps) to use
	//   the compatible custom types.
	spec_version: 100,
	impl_version: 1,
	apis: RUNTIME_API_VERSIONS,
	transaction_version: 1,
	state_version: 1,
};

/// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
	NativeVersion { runtime_version: VERSION, can_author_with: Default::default() }
}

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

/// We assume that ~10% of the block weight is consumed by `on_initialize` handlers.
/// This is used to limit the maximal weight of a single extrinsic.
const AVERAGE_ON_INITIALIZE_RATIO: Perbill = Perbill::from_percent(10);

/// We allow for 2 seconds of compute with a 6 second average block time.
const MAXIMUM_BLOCK_WEIGHT: Weight = WEIGHT_PER_SECOND.saturating_mul(2);

// Prints debug output of the `contracts` pallet to stdout if the node is
// started with `-lruntime::contracts=debug`.
const CONTRACTS_DEBUG_OUTPUT: bool = true;

// Unit = the base number of indivisible units for balances
const UNIT: Balance = 1_000_000_000_000;
const MILLIUNIT: Balance = 1_000_000_000;
pub const EXISTENTIAL_DEPOSIT: Balance = MILLIUNIT;

const fn deposit(items: u32, bytes: u32) -> Balance {
	(items as Balance * UNIT + (bytes as Balance) * (5 * MILLIUNIT / 100)) / 10
}

parameter_types! {
	pub const BlockHashCount: BlockNumber = 2400;
	pub const Version: RuntimeVersion = VERSION;

	// This part is copied from Substrate's `bin/node/runtime/src/lib.rs`.
	//  The `RuntimeBlockLength` and `RuntimeBlockWeights` exist here because the
	// `DeletionWeightLimit` and `DeletionQueueDepth` depend on those to parameterize
	// the lazy contract deletion.
	pub RuntimeBlockLength: BlockLength =
		BlockLength::max_with_normal_ratio(5 * 1024 * 1024, NORMAL_DISPATCH_RATIO);
	pub RuntimeBlockWeights: BlockWeights = BlockWeights::builder()
		.base_block(BlockExecutionWeight::get())
		.for_class(DispatchClass::all(), |weights| {
			weights.base_extrinsic = ExtrinsicBaseWeight::get();
		})
		.for_class(DispatchClass::Normal, |weights| {
			weights.max_total = Some(NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT);
		})
		.for_class(DispatchClass::Operational, |weights| {
			weights.max_total = Some(MAXIMUM_BLOCK_WEIGHT);
			// Operational transactions have some extra reserved space, so that they
			// are included even if block reached `MAXIMUM_BLOCK_WEIGHT`.
			weights.reserved = Some(
				MAXIMUM_BLOCK_WEIGHT - NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT
			);
		})
		.avg_block_initialization(AVERAGE_ON_INITIALIZE_RATIO)
		.build_or_panic();

	pub const SS58Prefix: u8 = 42;
}

// Configure FRAME pallets to include in runtime.

impl frame_system::Config for Runtime {
	/// The basic call filter to use in dispatchable.
	type BaseCallFilter = frame_support::traits::Everything;
	/// Block & extrinsics weights: base values and limits.
	type BlockWeights = RuntimeBlockWeights;
	/// The maximum length of a block (in bytes).
	type BlockLength = RuntimeBlockLength;
	/// The identifier used to distinguish between accounts.
	type AccountId = AccountId;
	/// The aggregated dispatch type that is available for extrinsics.
	type Call = Call;
	/// The lookup mechanism to get account ID from whatever is passed in dispatchers.
	type Lookup = AccountIdLookup<AccountId, ()>;
	/// The index type for storing how many extrinsics an account has signed.
	type Index = Index;
	/// The index type for blocks.
	type BlockNumber = BlockNumber;
	/// The type for hashing blocks and tries.
	type Hash = Hash;
	/// The hashing algorithm used.
	type Hashing = BlakeTwo256;
	/// The header type.
	type Header = generic::Header<BlockNumber, BlakeTwo256>;
	/// The ubiquitous event type.
	type Event = Event;
	/// The ubiquitous origin type.
	type Origin = Origin;
	/// Maximum number of block number to block hash mappings to keep (oldest pruned first).
	type BlockHashCount = BlockHashCount;
	/// The weight of database operations that the runtime can invoke.
	type DbWeight = RocksDbWeight;
	/// Version of the runtime.
	type Version = Version;
	/// Converts a module to the index of the module in `construct_runtime!`.
	///
	/// This type is being generated by `construct_runtime!`.
	type PalletInfo = PalletInfo;
	/// What to do if a new account is created.
	type OnNewAccount = ();
	/// What to do if an account is fully reaped from the system.
	type OnKilledAccount = ();
	/// The data to be stored in an account.
	type AccountData = pallet_balances::AccountData<Balance>;
	/// Weight information for the extrinsics of this pallet.
	type SystemWeightInfo = ();
	/// This is used as an identifier of the chain. 42 is the generic substrate prefix.
	type SS58Prefix = SS58Prefix;
	/// The set code logic, just the default since we're not a parachain.
	type OnSetCode = ();
	type MaxConsumers = ConstU32<16>;
}

parameter_types! {
	pub const UncleGenerations: u32 = 0;
}

impl pallet_authorship::Config for Runtime {
	type FindAuthor = ();
	type UncleGenerations = UncleGenerations;
	type FilterUncle = ();
	type EventHandler = ();
}

impl pallet_randomness_collective_flip::Config for Runtime {}

parameter_types! {
	pub const MinimumPeriod: u64 = 5;
}

impl pallet_timestamp::Config for Runtime {
	/// A timestamp: milliseconds since the unix epoch.
	type Moment = u64;
	type OnTimestampSet = ();
	type MinimumPeriod = MinimumPeriod;
	type WeightInfo = ();
}

parameter_types! {
	pub const MaxLocks: u32 = 50;
}

impl pallet_balances::Config for Runtime {
	type MaxLocks = MaxLocks;
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
	/// The type for recording an account's balance.
	type Balance = Balance;
	/// The ubiquitous event type.
	type Event = Event;
	type DustRemoval = ();
	type ExistentialDeposit = ConstU128<EXISTENTIAL_DEPOSIT>;
	type AccountStore = System;
	type WeightInfo = pallet_balances::weights::SubstrateWeight<Runtime>;
}

impl pallet_transaction_payment::Config for Runtime {
	type Event = Event;
	type OnChargeTransaction = CurrencyAdapter<Balances, ()>;
	type OperationalFeeMultiplier = ConstU8<5>;
	type WeightToFee = IdentityFee<Balance>;
	type LengthToFee = IdentityFee<Balance>;
	type FeeMultiplierUpdate = ();
}

impl pallet_sudo::Config for Runtime {
	type Event = Event;
	type Call = Call;
}



/*____________________________________________________________________________________________________*/
use frame_support::{
	log::{error, trace},
	pallet_prelude::*,
	traits::fungibles::{
		approvals::{Inspect as AllowanceInspect, Mutate as AllowanceMutate},
		Inspect, InspectMetadata, Transfer, Mutate, Create, MutateHold,
		metadata::{Mutate as MetadataMutate, Inspect as OtherInspectMetadata}
	},
};
use sp_std::vec::Vec;

use pallet_contracts::chain_extension::{
    ChainExtension,
    Environment,
    Ext,
    InitState,
    RetVal,
    SysConfig,
    UncheckedFrom,
};

use sp_runtime::DispatchError;
use sp_runtime::TokenError;
use sp_runtime::ArithmeticError;
#[derive(Default)]
pub struct Psp22AssetExtension;


#[derive(Debug, Copy, Clone, PartialEq, Eq, Encode, Decode,  MaxEncodedLen)]
enum OriginType{
	Caller, 
	Address
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Encode, Decode, MaxEncodedLen)]
struct PalletAssetRequest{
	origin_type: OriginType,
	asset_id : u32, 
	target_address : [u8; 32], 
	amount : u128
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Encode, Decode, MaxEncodedLen)]
struct PalletAssetBalanceRequest{
	asset_id : u32, 
	address : [u8; 32], 
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Encode, Decode, MaxEncodedLen)]
pub enum PalletAssetErr {
	/// Some error occurred.
    Other,
    /// Failed to lookup some data.
	CannotLookup,
	/// A bad origin.
	BadOrigin,
	/// A custom error in a module.
	Module,
	/// At least one consumer is remaining so the account cannot be destroyed.
	ConsumerRemaining,
	/// There are no providers so the account cannot be created.
	NoProviders,
	/// There are too many consumers so the account cannot be created.
	TooManyConsumers,
	/// An error to do with tokens.
	Token(PalletAssetTokenErr),
	/// An arithmetic error.
	Arithmetic(PalletAssetArithmeticErr),
	//unknown error
    Unknown,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Encode, Decode, MaxEncodedLen)]
pub enum PalletAssetArithmeticErr {
	/// Underflow.
	Underflow,
	/// Overflow.
	Overflow,
	/// Division by zero.
	DivisionByZero,
	//unknown error
    Unknown,

}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Encode, Decode, MaxEncodedLen)]
pub enum PalletAssetTokenErr {
	/// Funds are unavailable.
	NoFunds,
	/// Account that must exist would die.
	WouldDie,
	/// Account cannot exist with the funds that would be given.
	BelowMinimum,
	/// Account cannot be created.
	CannotCreate,
	/// The asset in question is unknown.
	UnknownAsset,
	/// Funds exist but are frozen.
	Frozen,
	/// Operation is not supported by the asset.
	Unsupported,
	//unknown error
    Unknown,
}

impl From<DispatchError> for PalletAssetErr {
    fn from(e: DispatchError) -> Self {
        match e{
			DispatchError::Other(_) => PalletAssetErr::Other,
			DispatchError::CannotLookup => PalletAssetErr::CannotLookup,
			DispatchError::BadOrigin => PalletAssetErr::BadOrigin,
			DispatchError::Module(_) => PalletAssetErr::Module,
			DispatchError::ConsumerRemaining => PalletAssetErr::ConsumerRemaining,
			DispatchError::NoProviders => PalletAssetErr::NoProviders,
			DispatchError::TooManyConsumers => PalletAssetErr::TooManyConsumers,
			DispatchError::Token(token_err) => PalletAssetErr::Token(PalletAssetTokenErr::from(token_err)),
			DispatchError::Arithmetic(arithmetic_error) => PalletAssetErr::Arithmetic(PalletAssetArithmeticErr::from(arithmetic_error)),
			_ => PalletAssetErr::Unknown,
		}
    }
}

impl From<ArithmeticError> for PalletAssetArithmeticErr {
    fn from(e: ArithmeticError) -> Self {
        match e{
			ArithmeticError::Underflow => PalletAssetArithmeticErr::Underflow,
			ArithmeticError::Overflow => PalletAssetArithmeticErr::Overflow,
			ArithmeticError::DivisionByZero => PalletAssetArithmeticErr::DivisionByZero,
			_ => PalletAssetArithmeticErr::Unknown,
		}
    }
}

impl From<TokenError> for PalletAssetTokenErr {
    fn from(e: TokenError) -> Self {
        match e{
			TokenError::NoFunds => PalletAssetTokenErr::NoFunds,
			TokenError::WouldDie => PalletAssetTokenErr::WouldDie,
			TokenError::BelowMinimum => PalletAssetTokenErr::BelowMinimum,
			TokenError::CannotCreate => PalletAssetTokenErr::CannotCreate,
			TokenError::UnknownAsset => PalletAssetTokenErr::UnknownAsset,
			TokenError::Frozen => PalletAssetTokenErr::Frozen,
			TokenError::Unsupported => PalletAssetTokenErr::Unsupported,
			_ => PalletAssetTokenErr::Unknown,
		}
    }
}

type PSP22Result = Result::<(),PalletAssetErr>;

impl<T> ChainExtension<T> for Psp22AssetExtension
 where
 	T: SysConfig + pallet_assets::Config + pallet_contracts::Config,
 	<T as SysConfig>::AccountId: UncheckedFrom<<T as SysConfig>::Hash> + AsRef<[u8]>,
 {
 	fn call<E: Ext>(&mut self, mut env: Environment<E, InitState>) -> Result<RetVal, DispatchError>
 	where
 		E: Ext<T = T>,
 		<E::T as SysConfig>::AccountId: UncheckedFrom<<E::T as SysConfig>::Hash> + AsRef<[u8]>,
 	{
		let func_id = env.func_id();

        match func_id {

			//create
			1102 => {
                let mut env = env.buf_in_buf_out();
                let create_asset: (OriginType, T::AssetId, T::AccountId, T::Balance) = env.read_as()?;
				let (origin_id, asset_id, account_id, balance) = create_asset;
				let create_result = <pallet_assets::Pallet<T> as Create<T::AccountId>>::
					create(asset_id, account_id, true, balance);

				match create_result {
					DispatchResult::Ok(_) => {
					}
					DispatchResult::Err(e) => {
						let err = Result::<(),PalletAssetErr>::Err(PalletAssetErr::from(e));
						env.write(&err.encode(), false, None).map_err(|_| {
							DispatchError::Other("ChainExtension failed to call 'approve transfer'")
						})?;
					}
				}

            }

			//mint
			1103 => {
				let ext = env.ext();
                let mut env = env.buf_in_buf_out();
                let create_asset: (OriginType, T::AssetId, T::AccountId, T::Balance) = env.read_as()?;
				let (origin_id, asset_id, account_id, balance) = create_asset;

				if origin_id == OriginType::Caller
				{
					// let address_account = AccountId::decode(&mut ext.address().as_ref()).unwrap();
					// pallet_assets::Pallet::<Runtime>::transfer_ownership(Origin::signed(address_account.clone()), asset_id, MultiAddress::Id(address_account.clone()))?;
				}
				
				let create_result = <pallet_assets::Pallet<T> as Mutate<T::AccountId>>::
					mint_into(asset_id, &account_id, balance);


				match create_result {
					DispatchResult::Ok(_) => {},
					DispatchResult::Err(e) => {
						let err = PSP22Result::Err(PalletAssetErr::from(e));
						env.write(&err.encode(), false, None).map_err(|_| {
							DispatchError::Other("ChainExtension failed to call mint")
						})?;
					}
				}
            }

			//burn
			1104 => {
				let ext = env.ext();
                let mut env = env.buf_in_buf_out();
                let create_asset: (OriginType, T::AssetId, T::AccountId, T::Balance) = env.read_as()?;
				let (origin_id, asset_id, account_id, balance) = create_asset;

				if origin_id == OriginType::Caller
				{
					// let address_account = AccountId::decode(&mut ext.address().as_ref()).unwrap();
					// pallet_assets::Pallet::<Runtime>::transfer_ownership(Origin::signed(address_account.clone()), asset_id, MultiAddress::Id(address_account.clone()))?;
				}
				
				let burn_result = <pallet_assets::Pallet<T> as Mutate<T::AccountId>>::
					burn_from(asset_id, &account_id, balance);

				// match burn_result {
				// 	DispatchResult::Ok(_) => {},
				// 	DispatchResult::Err(e) => {
				// 		// let err = PSP22Result::Err(PalletAssetErr::from(e));
				// 		// env.write(&err.encode(), false, None).map_err(|_| {
				// 		// 	DispatchError::Other("ChainExtension failed to call burn")
				// 		// })?;
				// 	}
				// }
				
            }

			//transfer
			1105 => {

				let ext = env.ext();
				let address = ext.address().clone();
				let caller = ext.caller().clone();
                let mut env = env.buf_in_buf_out();
                let create_asset: (OriginType, T::AssetId, T::AccountId, T::Balance) = env.read_as()?;
				let (origin_id, asset_id, account_id, balance) = create_asset;

				let address_account;
				if origin_id == OriginType::Caller
				{
					address_account = address;
					// let a = AccountId::decode(&mut ext.address().as_ref()).unwrap();
					// pallet_assets::Pallet::<Runtime>::transfer_ownership(Origin::signed(a.clone()), asset_id, MultiAddress::Id(a.clone()))?;
				}
				else{
					address_account = caller;
				}
				
				let result = <pallet_assets::Pallet<T> as Transfer<T::AccountId>>::
					transfer(asset_id, &address_account, &account_id, balance, true);


				// match result {
				// 	DispatchResult::Ok(_) => {},
				// 	DispatchResult::Err(e) => {
				// 		// let err = PSP22Result::Err(PalletAssetErr::from(e));
				// 		// env.write(&err.encode(), false, None).map_err(|_| {
				// 		// 	DispatchError::Other("ChainExtension failed to call transfer")
				// 		// })?;
				// 	}
				// }
            }
			
			//balance
			1106 => {
				let ext = env.ext();
                let mut env = env.buf_in_buf_out();
				let create_asset: (T::AssetId, T::AccountId) = env.read_as()?;
				let (asset_id, account_id) = create_asset;
				let balance = <pallet_assets::Pallet<T> as Inspect<T::AccountId>>::
						balance(asset_id, &account_id);

                env.write(&balance.encode(), false, None).map_err(|_| {
                    DispatchError::Other("ChainExtension failed to call balance")
                })?;
            }

			//total_supply
			1107 => {
                let mut env = env.buf_in_buf_out();
                let asset_id: T::AssetId = env.read_as()?;
				let total_supply : T::Balance = pallet_assets::Pallet::<T>::total_supply(asset_id);
                env.write(&total_supply.encode(), false, None).map_err(|_| {
                    DispatchError::Other("ChainExtension failed to call total_supply")
                })?;
			}

			//approve_transfer
			1108 => {
				let ext = env.ext();
				let address = ext.address().clone();
				let caller = ext.caller().clone();
                let mut env = env.buf_in_buf_out();
                let create_asset: (OriginType, T::AssetId, T::AccountId, T::Balance) = env.read_as()?;
				let (origin_type, asset, to, amount) = create_asset;

				let from;
				if origin_type == OriginType::Caller
				{
					from = address;
					// let a = AccountId::decode(&mut ext.address().as_ref()).unwrap();
					// pallet_assets::Pallet::<Runtime>::transfer_ownership(Origin::signed(a.clone()), asset_id, MultiAddress::Id(a.clone()))?;
				}
				else{
					from = caller;
				}
				let result = <pallet_assets::Pallet::<T> as AllowanceMutate<T::AccountId>>::
									approve(asset, &from, &to, amount);
				match result {
					DispatchResult::Ok(_) => {
					}
					DispatchResult::Err(e) => {
						let err = Result::<(),PalletAssetErr>::Err(PalletAssetErr::from(e));
						env.write(&err.encode(), false, None).map_err(|_| {
							DispatchError::Other("ChainExtension failed to call 'approve'")
						})?;
					}
				}
            }

			//transfer_approved
			1109 => {
				let ext = env.ext();
				let address = ext.address().clone();
				let caller = ext.caller().clone();
                let mut env = env.buf_in_buf_out();
                let create_asset: (T::AccountId, (OriginType, T::AssetId, T::AccountId, T::Balance)) = env.read_as()?;
				let owner = create_asset.0;
				let (origin_type, asset, to, amount) = create_asset.1;

				let from;
				if origin_type == OriginType::Caller
				{
					from = address;
					// let a = AccountId::decode(&mut ext.address().as_ref()).unwrap();
					// pallet_assets::Pallet::<Runtime>::transfer_ownership(Origin::signed(a.clone()), asset_id, MultiAddress::Id(a.clone()))?;
				}
				else{
					from = caller;
				}
				let result = <pallet_assets::Pallet::<T> as AllowanceMutate<T::AccountId>>::
					transfer_from(asset, &from, &owner, &to, amount);
				match result {
					DispatchResult::Ok(_) => {
					}
					DispatchResult::Err(e) => {
						let err = Result::<(),PalletAssetErr>::Err(PalletAssetErr::from(e));
						env.write(&err.encode(), false, None).map_err(|_| {
							DispatchError::Other("ChainExtension failed to call 'approved transfer'")
						})?;
					}
				}
            }

			//allowance
			1110 => {
                let mut env = env.buf_in_buf_out();
                let allowance_request: (T::AssetId, T::AccountId, T::AccountId) = env.read_as()?;

				let allowance = <pallet_assets::Pallet<T> as AllowanceInspect<T::AccountId>>
					::allowance(allowance_request.0, &allowance_request.1, &allowance_request.2);

                env.write(&allowance.encode(), false, None).map_err(|_| {
                    DispatchError::Other("ChainExtension failed to call balance")
                })?;
            }

			
			//increase_allowance/decrease_allowance
			1111 => {
				use frame_support::dispatch::DispatchResult;
                let mut env = env.buf_in_buf_out();
                let request: (u32, [u8; 32], [u8; 32], u128, bool) = env.read_as()?;
				let (asset_id, owner, delegate, amount, is_increase) = request;
				let mut vec = &owner.to_vec()[..];
				let owner_address = AccountId::decode(&mut vec).unwrap();
				let mut vec = &delegate.to_vec()[..];
				let delegate_address = AccountId::decode(&mut vec).unwrap();
				
				use crate::sp_api_hidden_includes_construct_runtime::hidden_include::traits::fungibles::approvals::Inspect;
                let allowance :u128 = Assets::allowance(asset_id, &owner_address, &delegate_address);
				let new_allowance = 
				if is_increase {allowance + amount} 
				else {
					if allowance < amount  { 0 }
					else {allowance - amount}
				};
				let cancel_approval_result = pallet_assets::Pallet::<Runtime>::
				cancel_approval(Origin::signed(owner_address.clone()),
				asset_id, 
				MultiAddress::Id(delegate_address.clone()));
				match cancel_approval_result {
					DispatchResult::Ok(_) => {
						error!("OK cancel_approval")
					}
					DispatchResult::Err(e) => {
						error!("ERROR cancel_approval");
						error!("{:#?}", e);
						let err = Result::<(),PalletAssetErr>::Err(PalletAssetErr::from(e));
						env.write(&err.encode(), false, None).map_err(|_| {
							DispatchError::Other("ChainExtension failed to call 'approve transfer'")
						})?;
					}
				}
				if cancel_approval_result.is_ok(){
					let approve_transfer_result = pallet_assets::Pallet::<Runtime>::
					approve_transfer(Origin::signed(owner_address),
					asset_id, 
					MultiAddress::Id(delegate_address), 
					new_allowance);
					error!("old allowance {}", allowance);
					error!("new allowance {}", new_allowance);
					error!("increase_allowance input {:#?}", request);
					error!("increase_allowance output {:#?}", approve_transfer_result);
					match approve_transfer_result {
						DispatchResult::Ok(_) => {
							error!("OK increase_allowance")
						}
						DispatchResult::Err(e) => {
							error!("ERROR increase_allowance");
							error!("{:#?}", e);
							let err = Result::<(),PalletAssetErr>::Err(PalletAssetErr::from(e));
							env.write(&err.encode(), false, None).map_err(|_| {
								DispatchError::Other("ChainExtension failed to call 'approve transfer'")
							})?;
						}
					}
				}
            }
			

			//set_metadata
			1112 => {
				let ext = env.ext();
				let address = ext.address().clone();
				let caller = ext.caller().clone();
                let mut env = env.buf_in_buf_out();
                let input: (OriginType, T::AssetId, Vec<u8>, Vec<u8>, u8) = env.read_as_unbounded(env.in_len())?;
				let (origin_type, asset_id, name, symbol, decimals) = input;
				
				let from = if origin_type == OriginType::Caller { address } else{ caller };
				
				let result = <pallet_assets::Pallet::<T> as MetadataMutate<T::AccountId>>::
					set(asset_id, &from, name, symbol, decimals);

				match result {
					DispatchResult::Ok(_) => {},
					DispatchResult::Err(e) => {
						let err = PSP22Result::Err(PalletAssetErr::from(e));
						env.write(&err.encode(), false, None).map_err(|_| {
							DispatchError::Other("ChainExtension failed to call set_metadata")
						})?;
					}
				}
			}

			//get asset metadata name
			1113 => {
                let mut env = env.buf_in_buf_out();
                let asset_id = env.read_as()?;
				let name = <pallet_assets::Pallet<T> as InspectMetadata<T::AccountId>>::name(&asset_id).encode();
                env.write(&name.encode()[..], false, None).map_err(|_| {
                    DispatchError::Other("ChainExtension failed to call get metadata name")
                })?;
			}

			//get asset metadata symbol
			1114 => {
                let mut env = env.buf_in_buf_out();
                let asset_id = env.read_as()?;
				let symbol = <pallet_assets::Pallet<T> as InspectMetadata<T::AccountId>>::symbol(&asset_id);
                env.write(&symbol.encode()[..], false, None).map_err(|_| {
                    DispatchError::Other("ChainExtension failed to call balance")
                })?;
			}

			//decimals
			1115 => {
                let mut env = env.buf_in_buf_out();
                let asset_id = env.read_as()?;
				let decimals = <pallet_assets::Pallet<T> as InspectMetadata<T::AccountId>>::decimals(&asset_id,);
                env.write(&decimals.encode()[..], false, None).map_err(|_| {
                    DispatchError::Other("ChainExtension failed to call total_supply")
                })?;
			}
            _ => {
                error!("Called an unregistered `func_id`: {:}", func_id);
                return Err(DispatchError::Other("Unimplemented func_id"))
            }
        }

        Ok(RetVal::Converging(0))

		
    }

    fn enabled() -> bool {
        true
    }
}
/*____________________________________________________________________________________________________*/



parameter_types! {
	pub const DepositPerItem: Balance = deposit(1, 0);
	pub const DepositPerByte: Balance = deposit(0, 1);
	pub const DeletionQueueDepth: u32 = 128;
	// The lazy deletion runs inside on_initialize.
	pub DeletionWeightLimit: Weight = RuntimeBlockWeights::get()
		.per_class
		.get(DispatchClass::Normal)
		.max_total
		.unwrap_or(RuntimeBlockWeights::get().max_block);
	pub Schedule: pallet_contracts::Schedule<Runtime> = Default::default();
}

impl pallet_contracts::Config for Runtime {
	type Time = Timestamp;
	type Randomness = RandomnessCollectiveFlip;
	type Currency = Balances;
	type Event = Event;
	type Call = Call;
	/// The safest default is to allow no calls at all.
	///
	/// Runtimes should whitelist dispatchables that are allowed to be called from contracts
	/// and make sure they are stable. Dispatchables exposed to contracts are not allowed to
	/// change because that would break already deployed contracts. The `Call` structure itself
	/// is not allowed to change the indices of existing pallets, too.
	type CallFilter = frame_support::traits::Nothing;
	type DepositPerItem = DepositPerItem;
	type DepositPerByte = DepositPerByte;
	type CallStack = [pallet_contracts::Frame<Self>; 31];
	type WeightPrice = pallet_transaction_payment::Pallet<Self>;
	type WeightInfo = pallet_contracts::weights::SubstrateWeight<Self>;
	type ChainExtension = Psp22AssetExtension;
	type DeletionQueueDepth = DeletionQueueDepth;
	type DeletionWeightLimit = DeletionWeightLimit;
	type Schedule = Schedule;
	type AddressGenerator = pallet_contracts::DefaultAddressGenerator;
	type ContractAccessWeight = DefaultContractAccessWeight<RuntimeBlockWeights>;
	// This node is geared towards development and testing of contracts.
	// We decided to increase the default allowed contract size for this
	// reason (the default is `128 * 1024`).
	//
	// Our reasoning is that the error code `CodeTooLarge` is thrown
	// if a too-large contract is uploaded. We noticed that it poses
	// less friction during development when the requirement here is
	// just more lax.
	type MaxCodeLen = ConstU32<{ 256 * 1024 }>;
	type RelaxedMaxCodeLen = ConstU32<{ 512 * 1024 }>;
	type MaxStorageKeyLen = ConstU32<128>;
}

pub type CurrencyAssetId = u32;
pub const MILLICENTS: Balance = 10_000_000;
pub const CENTS: Balance = 1_000 * MILLICENTS; // assume this is worth about a cent.
pub const DOLLARS: Balance = 100 * CENTS;

pub const EXISTENTIAL_DEPOSIT_ASSETS: u128 = 10 * CENTS; // 0.1 Native Token Balance

pub const fn deposit2(items: u32, bytes: u32) -> Balance {
	items as Balance * 15 * CENTS + (bytes as Balance) * 6 * CENTS
}
parameter_types! {
    pub const AssetDeposit: Balance = DOLLARS; // 1 UNIT deposit to create asset
    pub const ApprovalDeposit: Balance = EXISTENTIAL_DEPOSIT_ASSETS;
    pub const AssetAccountDeposit: Balance = deposit2(1, 16);
    pub const AssetsStringLimit: u32 = 50;
    pub const MetadataDepositBase: Balance = deposit2(1, 68);
    pub const MetadataDepositPerByte: Balance = deposit2(0, 1);
}

impl pallet_assets::Config for Runtime {
    type Event = Event;
    type Balance = Balance;
    type AssetId = CurrencyAssetId;
    type Currency = Balances;
    type ForceOrigin = EnsureRoot<AccountId>;
    type AssetDeposit = AssetDeposit;
    type MetadataDepositBase = MetadataDepositBase;
    type MetadataDepositPerByte = MetadataDepositPerByte;
    type AssetAccountDeposit = AssetAccountDeposit;
    type ApprovalDeposit = ApprovalDeposit;
    type StringLimit = AssetsStringLimit;
    type Freezer = ();
    type WeightInfo = ();
    type Extra = ();
}

pub struct Migrations;
impl OnRuntimeUpgrade for Migrations {
	fn on_runtime_upgrade() -> Weight {
		migration::migrate::<Runtime>()
	}
}

// Create the runtime by composing the FRAME pallets that were previously configured.
construct_runtime!(
	pub struct Runtime
	where
		Block = Block,
		NodeBlock = opaque::Block,
		UncheckedExtrinsic = UncheckedExtrinsic,
	{
		System: frame_system,
		RandomnessCollectiveFlip: pallet_randomness_collective_flip,
		Timestamp: pallet_timestamp,
		Balances: pallet_balances,
		Authorship: pallet_authorship,
		TransactionPayment: pallet_transaction_payment,
		Sudo: pallet_sudo,
		Contracts: pallet_contracts,
		Assets: pallet_assets,
	}
);

/// The address format for describing accounts.
pub type Address = sp_runtime::MultiAddress<AccountId, ()>;
/// Block header type as expected by this runtime.
pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;
/// The SignedExtension to the basic transaction logic.
pub type SignedExtra = (
	frame_system::CheckNonZeroSender<Runtime>,
	frame_system::CheckSpecVersion<Runtime>,
	frame_system::CheckTxVersion<Runtime>,
	frame_system::CheckGenesis<Runtime>,
	frame_system::CheckEra<Runtime>,
	frame_system::CheckNonce<Runtime>,
	frame_system::CheckWeight<Runtime>,
	pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
);
/// The payload being signed in transactions.
pub type SignedPayload = generic::SignedPayload<Call, SignedExtra>;
/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic = generic::UncheckedExtrinsic<Address, Call, Signature, SignedExtra>;
/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
	Runtime,
	Block,
	frame_system::ChainContext<Runtime>,
	Runtime,
	AllPalletsWithSystem,
	Migrations,
>;

impl_runtime_apis! {
	impl sp_api::Core<Block> for Runtime {
		fn version() -> RuntimeVersion {
			VERSION
		}

		fn execute_block(block: Block) {
			Executive::execute_block(block);
		}

		fn initialize_block(header: &<Block as BlockT>::Header) {
			Executive::initialize_block(header)
		}
	}

	impl sp_api::Metadata<Block> for Runtime {
		fn metadata() -> OpaqueMetadata {
			OpaqueMetadata::new(Runtime::metadata().into())
		}
	}

	impl sp_block_builder::BlockBuilder<Block> for Runtime {
		fn apply_extrinsic(extrinsic: <Block as BlockT>::Extrinsic) -> ApplyExtrinsicResult {
			Executive::apply_extrinsic(extrinsic)
		}

		fn finalize_block() -> <Block as BlockT>::Header {
			Executive::finalize_block()
		}

		fn inherent_extrinsics(data: sp_inherents::InherentData) -> Vec<<Block as BlockT>::Extrinsic> {
			data.create_extrinsics()
		}

		fn check_inherents(
			block: Block,
			data: sp_inherents::InherentData,
		) -> sp_inherents::CheckInherentsResult {
			data.check_extrinsics(&block)
		}
	}

	impl sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block> for Runtime {
		fn validate_transaction(
			source: TransactionSource,
			tx: <Block as BlockT>::Extrinsic,
			block_hash: <Block as BlockT>::Hash,
		) -> TransactionValidity {
			Executive::validate_transaction(source, tx, block_hash)
		}
	}

	impl sp_offchain::OffchainWorkerApi<Block> for Runtime {
		fn offchain_worker(header: &<Block as BlockT>::Header) {
			Executive::offchain_worker(header)
		}
	}

	impl sp_session::SessionKeys<Block> for Runtime {
		fn generate_session_keys(seed: Option<Vec<u8>>) -> Vec<u8> {
			opaque::SessionKeys::generate(seed)
		}

		fn decode_session_keys(
			encoded: Vec<u8>,
		) -> Option<Vec<(Vec<u8>, KeyTypeId)>> {
			opaque::SessionKeys::decode_into_raw_public_keys(&encoded)
		}
	}

	impl frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Index> for Runtime {
		fn account_nonce(account: AccountId) -> Index {
			System::account_nonce(account)
		}
	}

	impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance> for Runtime {
		fn query_info(
			uxt: <Block as BlockT>::Extrinsic,
			len: u32,
		) -> pallet_transaction_payment_rpc_runtime_api::RuntimeDispatchInfo<Balance> {
			TransactionPayment::query_info(uxt, len)
		}
		fn query_fee_details(
			uxt: <Block as BlockT>::Extrinsic,
			len: u32,
		) -> pallet_transaction_payment::FeeDetails<Balance> {
			TransactionPayment::query_fee_details(uxt, len)
		}
	}

	impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentCallApi<Block, Balance, Call> for Runtime {
		fn query_call_info(
			call: Call,
			len: u32,
		) -> pallet_transaction_payment::RuntimeDispatchInfo<Balance> {
			TransactionPayment::query_call_info(call, len)
		}
		fn query_call_fee_details(
			call: Call,
			len: u32,
		) -> pallet_transaction_payment::FeeDetails<Balance> {
			TransactionPayment::query_call_fee_details(call, len)
		}
	}

	#[cfg(feature = "runtime-benchmarks")]
	impl frame_benchmarking::Benchmark<Block> for Runtime {
		fn benchmark_metadata(extra: bool) -> (
			Vec<frame_benchmarking::BenchmarkList>,
			Vec<frame_support::traits::StorageInfo>,
		) {
			use frame_benchmarking::{list_benchmark, baseline, Benchmarking, BenchmarkList};
			use frame_support::traits::StorageInfoTrait;
			use frame_system_benchmarking::Pallet as SystemBench;
			use baseline::Pallet as BaselineBench;

			let mut list = Vec::<BenchmarkList>::new();

			list_benchmark!(list, extra, frame_benchmarking, BaselineBench::<Runtime>);
			list_benchmark!(list, extra, frame_system, SystemBench::<Runtime>);
			list_benchmark!(list, extra, pallet_balances, Balances);
			list_benchmark!(list, extra, pallet_timestamp, Timestamp);

			let storage_info = AllPalletsWithSystem::storage_info();

			(list, storage_info)
		}

		fn dispatch_benchmark(
			config: frame_benchmarking::BenchmarkConfig
		) -> Result<Vec<frame_benchmarking::BenchmarkBatch>, sp_runtime::RuntimeString> {
			use frame_benchmarking::{baseline, Benchmarking, BenchmarkBatch, add_benchmark, TrackedStorageKey};

			use frame_system_benchmarking::Pallet as SystemBench;
			use baseline::Pallet as BaselineBench;

			impl frame_system_benchmarking::Config for Runtime {}
			impl baseline::Config for Runtime {}

			let whitelist: Vec<TrackedStorageKey> = vec![
				// Block Number
				hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef702a5c1b19ab7a04f536c519aca4983ac").to_vec().into(),
				// Total Issuance
				hex_literal::hex!("c2261276cc9d1f8598ea4b6a74b15c2f57c875e4cff74148e4628f264b974c80").to_vec().into(),
				// Execution Phase
				hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef7ff553b5a9862a516939d82b3d3d8661a").to_vec().into(),
				// Event Count
				hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef70a98fdbe9ce6c55837576c60c7af3850").to_vec().into(),
				// System Events
				hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef780d41e5e16056765bc8461851072c9d7").to_vec().into(),
			];

			let mut batches = Vec::<BenchmarkBatch>::new();
			let params = (&config, &whitelist);

			add_benchmark!(params, batches, frame_benchmarking, BaselineBench::<Runtime>);
			add_benchmark!(params, batches, frame_system, SystemBench::<Runtime>);
			add_benchmark!(params, batches, pallet_balances, Balances);
			add_benchmark!(params, batches, pallet_timestamp, Timestamp);

			Ok(batches)
		}
	}

	impl pallet_contracts_rpc_runtime_api::ContractsApi<Block, AccountId, Balance, BlockNumber, Hash>
		for Runtime
	{
		fn call(
			origin: AccountId,
			dest: AccountId,
			value: Balance,
			gas_limit: u64,
			storage_deposit_limit: Option<Balance>,
			input_data: Vec<u8>,
		) -> pallet_contracts_primitives::ContractExecResult<Balance> {
			Contracts::bare_call(origin, dest, value, Weight::from_ref_time(gas_limit), storage_deposit_limit, input_data, CONTRACTS_DEBUG_OUTPUT)
		}

		fn instantiate(
			origin: AccountId,
			value: Balance,
			gas_limit: u64,
			storage_deposit_limit: Option<Balance>,
			code: pallet_contracts_primitives::Code<Hash>,
			data: Vec<u8>,
			salt: Vec<u8>,
		) -> pallet_contracts_primitives::ContractInstantiateResult<AccountId, Balance>
		{
			Contracts::bare_instantiate(origin, value,  Weight::from_ref_time(gas_limit), storage_deposit_limit, code, data, salt, CONTRACTS_DEBUG_OUTPUT)
		}

		fn upload_code(
			origin: AccountId,
			code: Vec<u8>,
			storage_deposit_limit: Option<Balance>,
		) -> pallet_contracts_primitives::CodeUploadResult<Hash, Balance>
		{
			Contracts::bare_upload_code(origin, code, storage_deposit_limit)
		}

		fn get_storage(
			address: AccountId,
			key: Vec<u8>,
		) -> pallet_contracts_primitives::GetStorageResult {
			Contracts::get_storage(address, key)
		}
	}

	#[cfg(feature = "try-runtime")]
	impl frame_try_runtime::TryRuntime<Block> for Runtime {
		fn on_runtime_upgrade() -> (Weight, Weight) {
			// NOTE: intentional unwrap: we don't want to propagate the error backwards, and want to
			// have a backtrace here. If any of the pre/post migration checks fail, we shall stop
			// right here and right now.
			let weight = Executive::try_runtime_upgrade().unwrap();
			(weight, BlockWeights::get().max_block)
		}

		fn execute_block(
			block: Block,
			state_root_check: bool,
			select: frame_try_runtime::TryStateSelect
		) -> Weight {
			// NOTE: intentional unwrap: we don't want to propagate the error backwards, and want to
			// have a backtrace here.
			Executive::try_execute_block(block, state_root_check, select).expect("execute-block failed")
		}
	}
}
