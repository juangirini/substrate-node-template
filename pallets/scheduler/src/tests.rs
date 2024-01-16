// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! # Scheduler tests.

use super::*;
use crate::mock::{
	logger, new_test_ext, root, run_to_block, LoggerCall, RuntimeCall, Scheduler, Preimage, Test, *,
};
use frame_support::{
	assert_err, assert_noop, assert_ok,
	traits::{
		Contains, 
		OnInitialize, QueryPreimage, 
		StorePreimage, ConstU32},
	Hashable,
};
use sp_runtime::traits::Hash;
use substrate_test_utils::assert_eq_uvec;

use ark_std::{
	rand::SeedableRng,
	ops::Mul,
	One,
};
use ark_bls12_381::{Fr, G2Projective as G2};
use ark_ec::Group;
use etf_crypto_primitives::utils::convert_to_bytes;
use etf_crypto_primitives::{
	client::etf_client::{
		DefaultEtfClient, 
		EtfClient
	},
	ibe::fullident::BfIbe,
};
use rand_chacha::ChaCha20Rng;

#[test]
#[docify::export]
fn basic_scheduling_works() {
	new_test_ext().execute_with(|| {
		// Call to schedule
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// BaseCallFilter should be implemented to accept `Logger::log` runtime call which is
		// implemented for `BaseFilter` in the mock runtime
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));

		// Schedule call to be executed at the 4th block
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap()
		));

		// `log` runtime call should not have executed yet
		run_to_block(3);
		assert!(logger::log().is_empty());

		run_to_block(4);
		// `log` runtime call should have executed at block 4
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
#[docify::export]
fn scheduling_with_preimages_works() {
	new_test_ext().execute_with(|| {
		// Call to schedule
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		let hash = <Test as frame_system::Config>::Hashing::hash_of(&call);
		let len = call.using_encoded(|x| x.len()) as u32;

		// Important to use here `Bounded::Lookup` to ensure that that the Scheduler can request the
		// hash from PreImage to dispatch the call
		let hashed = Bounded::Lookup { hash, len };

		// Schedule call to be executed at block 4 with the PreImage hash
		assert_ok!(Scheduler::do_schedule(DispatchTime::At(4), None, 127, root(), hashed));

		// Register preimage on chain
		assert_ok!(Preimage::note_preimage(RuntimeOrigin::signed(0), call.encode()));
		assert!(Preimage::is_requested(&hash));

		// `log` runtime call should not have executed yet
		run_to_block(3);
		assert!(logger::log().is_empty());

		run_to_block(4);
		// preimage should not have been removed when executed by the scheduler
		assert!(!Preimage::len(&hash).is_some());
		assert!(!Preimage::is_requested(&hash));
		// `log` runtime call should have executed at block 4
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn schedule_after_works() {
	new_test_ext().execute_with(|| {
		run_to_block(2);
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));
		// This will schedule the call 3 blocks after the next block... so block 3 + 3 = 6
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::After(3),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap()
		));
		run_to_block(5);
		assert!(logger::log().is_empty());
		run_to_block(6);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn schedule_after_zero_works() {
	new_test_ext().execute_with(|| {
		run_to_block(2);
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::After(0),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap()
		));
		// Will trigger on the next block.
		run_to_block(3);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn periodic_scheduling_works() {
	new_test_ext().execute_with(|| {
		// at #4, every 3 blocks, 3 times.
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			Some((3, 3)),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(logger::Call::log {
				i: 42,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap()
		));
		run_to_block(3);
		assert!(logger::log().is_empty());
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(6);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(7);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 42u32)]);
		run_to_block(9);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 42u32)]);
		run_to_block(10);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 42u32), (root(), 42u32)]);
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 42u32), (root(), 42u32)]);
	});
}

#[test]
fn reschedule_works() {
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));
		assert_eq!(
			Scheduler::do_schedule(
				DispatchTime::At(4),
				None,
				127,
				root(),
				Preimage::bound(call).unwrap()
			)
			.unwrap(),
			(4, 0)
		);

		run_to_block(3);
		assert!(logger::log().is_empty());

		assert_eq!(Scheduler::do_reschedule((4, 0), DispatchTime::At(6)).unwrap(), (6, 0));

		assert_noop!(
			Scheduler::do_reschedule((6, 0), DispatchTime::At(6)),
			Error::<Test>::RescheduleNoChange
		);

		run_to_block(4);
		assert!(logger::log().is_empty());

		run_to_block(6);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn reschedule_named_works() {
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));
		assert_eq!(
			Scheduler::do_schedule_named(
				[1u8; 32],
				DispatchTime::At(4),
				None,
				127,
				root(),
				Preimage::bound(call).unwrap(),
			)
			.unwrap(),
			(4, 0)
		);

		run_to_block(3);
		assert!(logger::log().is_empty());

		assert_eq!(Scheduler::do_reschedule_named([1u8; 32], DispatchTime::At(6)).unwrap(), (6, 0));

		assert_noop!(
			Scheduler::do_reschedule_named([1u8; 32], DispatchTime::At(6)),
			Error::<Test>::RescheduleNoChange
		);

		run_to_block(4);
		assert!(logger::log().is_empty());

		run_to_block(6);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn reschedule_named_perodic_works() {
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		assert!(!<Test as frame_system::Config>::BaseCallFilter::contains(&call));
		assert_eq!(
			Scheduler::do_schedule_named(
				[1u8; 32],
				DispatchTime::At(4),
				Some((3, 3)),
				127,
				root(),
				Preimage::bound(call).unwrap(),
			)
			.unwrap(),
			(4, 0)
		);

		run_to_block(3);
		assert!(logger::log().is_empty());

		assert_eq!(Scheduler::do_reschedule_named([1u8; 32], DispatchTime::At(5)).unwrap(), (5, 0));
		assert_eq!(Scheduler::do_reschedule_named([1u8; 32], DispatchTime::At(6)).unwrap(), (6, 0));

		run_to_block(5);
		assert!(logger::log().is_empty());

		run_to_block(6);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		assert_eq!(
			Scheduler::do_reschedule_named([1u8; 32], DispatchTime::At(10)).unwrap(),
			(10, 0)
		);

		run_to_block(9);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		run_to_block(10);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 42u32)]);

		run_to_block(13);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 42u32), (root(), 42u32)]);

		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 42u32), (root(), 42u32)]);
	});
}

#[test]
fn cancel_named_scheduling_works_with_normal_cancel() {
	new_test_ext().execute_with(|| {
		// at #4.
		Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
				i: 69,
				weight: Weight::from_parts(10, 0),
			}))
			.unwrap(),
		)
		.unwrap();
		let i = Scheduler::do_schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
				i: 42,
				weight: Weight::from_parts(10, 0),
			}))
			.unwrap(),
		)
		.unwrap();
		run_to_block(3);
		assert!(logger::log().is_empty());
		assert_ok!(Scheduler::do_cancel_named(None, [1u8; 32]));
		assert_ok!(Scheduler::do_cancel(None, i));
		run_to_block(100);
		assert!(logger::log().is_empty());
	});
}

#[test]
fn cancel_named_periodic_scheduling_works() {
	new_test_ext().execute_with(|| {
		// at #4, every 3 blocks, 3 times.
		Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(4),
			Some((3, 3)),
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
				i: 42,
				weight: Weight::from_parts(10, 0),
			}))
			.unwrap(),
		)
		.unwrap();
		// same id results in error.
		assert!(Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
				i: 69,
				weight: Weight::from_parts(10, 0)
			}))
			.unwrap(),
		)
		.is_err());
		// different id is ok.
		Scheduler::do_schedule_named(
			[2u8; 32],
			DispatchTime::At(8),
			None,
			127,
			root(),
			Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
				i: 69,
				weight: Weight::from_parts(10, 0),
			}))
			.unwrap(),
		)
		.unwrap();
		run_to_block(3);
		assert!(logger::log().is_empty());
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(6);
		assert_ok!(Scheduler::do_cancel_named(None, [1u8; 32]));
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 69u32)]);
	});
}

#[test]
fn scheduler_respects_weight_limits() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight / 3 * 2 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log { i: 69, weight: max_weight / 3 * 2 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		// 69 and 42 do not fit together
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		run_to_block(5);
		assert_eq!(logger::log(), vec![(root(), 42u32), (root(), 69u32)]);
	});
}

/// Permanently overweight calls are not deleted but also not executed.
#[test]
fn scheduler_does_not_delete_permanently_overweight_call() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		// Never executes.
		run_to_block(100);
		assert_eq!(logger::log(), vec![]);

		// Assert the `PermanentlyOverweight` event.
		assert_eq!(
			System::events().last().unwrap().event,
			crate::Event::PermanentlyOverweight { task: (4, 0), id: None }.into(),
		);
		// The call is still in the agenda.
		assert!(Agenda::<Test>::get(4)[0].is_some());
	});
}

#[test]
fn scheduler_handles_periodic_failure() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	let max_per_block = <Test as Config>::MaxScheduledPerBlock::get();

	new_test_ext().execute_with(|| {
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: (max_weight / 3) * 2 });
		let bound = Preimage::bound(call).unwrap();

		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			Some((4, u32::MAX)),
			127,
			root(),
			bound.clone(),
		));
		// Executes 5 times till block 20.
		run_to_block(20);
		assert_eq!(logger::log().len(), 5);

		// Block 28 will already be full.
		for _ in 0..max_per_block {
			assert_ok!(Scheduler::do_schedule(
				DispatchTime::At(28),
				None,
				120,
				root(),
				bound.clone(),
			));
		}

		// Going to block 24 will emit a `PeriodicFailed` event.
		run_to_block(24);
		assert_eq!(logger::log().len(), 6);

		assert_eq!(
			System::events().last().unwrap().event,
			crate::Event::PeriodicFailed { task: (24, 0), id: None }.into(),
		);
	});
}

#[test]
fn scheduler_handles_periodic_unavailable_preimage() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();

	new_test_ext().execute_with(|| {
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: (max_weight / 3) * 2 });
		let hash = <Test as frame_system::Config>::Hashing::hash_of(&call);
		let len = call.using_encoded(|x| x.len()) as u32;
		// Important to use here `Bounded::Lookup` to ensure that we request the hash.
		let bound = Bounded::Lookup { hash, len };
		// The preimage isn't requested yet.
		assert!(!Preimage::is_requested(&hash));

		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			Some((4, u32::MAX)),
			127,
			root(),
			bound.clone(),
		));

		// The preimage is requested.
		assert!(Preimage::is_requested(&hash));

		// Note the preimage.
		assert_ok!(Preimage::note_preimage(RuntimeOrigin::signed(1), call.encode()));

		// Executes 1 times till block 4.
		run_to_block(4);
		assert_eq!(logger::log().len(), 1);

		// Unnote the preimage
		Preimage::unnote(&hash);

		// Does not ever execute again.
		run_to_block(100);
		assert_eq!(logger::log().len(), 1);

		// The preimage is not requested anymore.
		assert!(!Preimage::is_requested(&hash));
	});
}

#[test]
fn scheduler_respects_priority_ordering() {
	let max_weight: Weight = <Test as Config>::MaximumWeight::get();
	new_test_ext().execute_with(|| {
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight / 3 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			None,
			1,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log { i: 69, weight: max_weight / 3 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			None,
			0,
			root(),
			Preimage::bound(call).unwrap(),
		));
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 69u32), (root(), 42u32)]);
	});
}

#[test]
fn scheduler_respects_priority_ordering_with_soft_deadlines() {
	new_test_ext().execute_with(|| {
		let max_weight: Weight = <Test as Config>::MaximumWeight::get();
		let call = RuntimeCall::Logger(LoggerCall::log { i: 42, weight: max_weight / 5 * 2 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			None,
			255,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log { i: 69, weight: max_weight / 5 * 2 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log { i: 2600, weight: max_weight / 5 * 4 });
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(4),
			None,
			126,
			root(),
			Preimage::bound(call).unwrap(),
		));

		// 2600 does not fit with 69 or 42, but has higher priority, so will go through
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 2600u32)]);
		// 69 and 42 fit together
		run_to_block(5);
		assert_eq!(logger::log(), vec![(root(), 2600u32), (root(), 69u32), (root(), 42u32)]);
	});
}

#[test]
fn on_initialize_weight_is_correct() {
	new_test_ext().execute_with(|| {
		let call_weight = Weight::from_parts(25, 0);

		// Named
		let call = RuntimeCall::Logger(LoggerCall::log {
			i: 3,
			weight: call_weight + Weight::from_parts(1, 0),
		});
		assert_ok!(Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(3),
			None,
			255,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: call_weight + Weight::from_parts(2, 0),
		});
		// Anon Periodic
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(2),
			Some((1000, 3)),
			128,
			root(),
			Preimage::bound(call).unwrap(),
		));
		let call = RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: call_weight + Weight::from_parts(3, 0),
		});
		// Anon
		assert_ok!(Scheduler::do_schedule(
			DispatchTime::At(2),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		));
		// Named Periodic
		let call = RuntimeCall::Logger(LoggerCall::log {
			i: 2600,
			weight: call_weight + Weight::from_parts(4, 0),
		});
		assert_ok!(Scheduler::do_schedule_named(
			[2u8; 32],
			DispatchTime::At(1),
			Some((1000, 3)),
			126,
			root(),
			Preimage::bound(call).unwrap(),
		));

		// Will include the named periodic only
		assert_eq!(
			Scheduler::on_initialize(1),
			TestWeightInfo::service_agendas_base() +
				TestWeightInfo::service_agenda_base(1) +
				<TestWeightInfo as MarginalWeightInfo>::service_task(None, true, true) +
				TestWeightInfo::execute_dispatch_unsigned() +
				call_weight + Weight::from_parts(4, 0)
		);
		assert_eq!(IncompleteSince::<Test>::get(), None);
		assert_eq!(logger::log(), vec![(root(), 2600u32)]);

		// Will include anon and anon periodic
		assert_eq!(
			Scheduler::on_initialize(2),
			TestWeightInfo::service_agendas_base() +
				TestWeightInfo::service_agenda_base(2) +
				<TestWeightInfo as MarginalWeightInfo>::service_task(None, false, true) +
				TestWeightInfo::execute_dispatch_unsigned() +
				call_weight + Weight::from_parts(3, 0) +
				<TestWeightInfo as MarginalWeightInfo>::service_task(None, false, false) +
				TestWeightInfo::execute_dispatch_unsigned() +
				call_weight + Weight::from_parts(2, 0)
		);
		assert_eq!(IncompleteSince::<Test>::get(), None);
		assert_eq!(logger::log(), vec![(root(), 2600u32), (root(), 69u32), (root(), 42u32)]);

		// Will include named only
		assert_eq!(
			Scheduler::on_initialize(3),
			TestWeightInfo::service_agendas_base() +
				TestWeightInfo::service_agenda_base(1) +
				<TestWeightInfo as MarginalWeightInfo>::service_task(None, true, false) +
				TestWeightInfo::execute_dispatch_unsigned() +
				call_weight + Weight::from_parts(1, 0)
		);
		assert_eq!(IncompleteSince::<Test>::get(), None);
		assert_eq!(
			logger::log(),
			vec![(root(), 2600u32), (root(), 69u32), (root(), 42u32), (root(), 3u32)]
		);

		// Will contain none
		let actual_weight = Scheduler::on_initialize(4);
		assert_eq!(
			actual_weight,
			TestWeightInfo::service_agendas_base() + TestWeightInfo::service_agenda_base(0)
		);
	});
}

#[test]
fn root_calls_works() {
	new_test_ext().execute_with(|| {
		let call = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: Weight::from_parts(10, 0),
		}));
		let call2 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));
		assert_ok!(
			Scheduler::schedule_named(RuntimeOrigin::root(), [1u8; 32], 4, None, 127, call,)
		);
		assert_ok!(Scheduler::schedule(RuntimeOrigin::root(), 4, None, 127, call2));
		run_to_block(3);
		// Scheduled calls are in the agenda.
		assert_eq!(Agenda::<Test>::get(4).len(), 2);
		assert!(logger::log().is_empty());
		assert_ok!(Scheduler::cancel_named(RuntimeOrigin::root(), [1u8; 32]));
		assert_ok!(Scheduler::cancel(RuntimeOrigin::root(), 4, 1));
		// Scheduled calls are made NONE, so should not effect state
		run_to_block(100);
		assert!(logger::log().is_empty());
	});
}

#[test]
fn fails_to_schedule_task_in_the_past() {
	new_test_ext().execute_with(|| {
		run_to_block(3);

		let call1 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: Weight::from_parts(10, 0),
		}));
		let call2 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));
		let call3 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));

		assert_noop!(
			Scheduler::schedule_named(RuntimeOrigin::root(), [1u8; 32], 2, None, 127, call1),
			Error::<Test>::TargetBlockNumberInPast,
		);

		assert_noop!(
			Scheduler::schedule(RuntimeOrigin::root(), 2, None, 127, call2),
			Error::<Test>::TargetBlockNumberInPast,
		);

		assert_noop!(
			Scheduler::schedule(RuntimeOrigin::root(), 3, None, 127, call3),
			Error::<Test>::TargetBlockNumberInPast,
		);
	});
}

#[test]
fn should_use_origin() {
	new_test_ext().execute_with(|| {
		let call = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: Weight::from_parts(10, 0),
		}));
		let call2 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));
		assert_ok!(Scheduler::schedule_named(
			system::RawOrigin::Signed(1).into(),
			[1u8; 32],
			4,
			None,
			127,
			call,
		));
		assert_ok!(Scheduler::schedule(system::RawOrigin::Signed(1).into(), 4, None, 127, call2,));
		run_to_block(3);
		// Scheduled calls are in the agenda.
		assert_eq!(Agenda::<Test>::get(4).len(), 2);
		assert!(logger::log().is_empty());
		assert_ok!(Scheduler::cancel_named(system::RawOrigin::Signed(1).into(), [1u8; 32]));
		assert_ok!(Scheduler::cancel(system::RawOrigin::Signed(1).into(), 4, 1));
		// Scheduled calls are made NONE, so should not effect state
		run_to_block(100);
		assert!(logger::log().is_empty());
	});
}

#[test]
fn should_check_origin() {
	new_test_ext().execute_with(|| {
		let call = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 69,
			weight: Weight::from_parts(10, 0),
		}));
		let call2 = Box::new(RuntimeCall::Logger(LoggerCall::log {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));
		assert_noop!(
			Scheduler::schedule_named(
				system::RawOrigin::Signed(2).into(),
				[1u8; 32],
				4,
				None,
				127,
				call
			),
			BadOrigin
		);
		assert_noop!(
			Scheduler::schedule(system::RawOrigin::Signed(2).into(), 4, None, 127, call2),
			BadOrigin
		);
	});
}

#[test]
fn should_check_origin_for_cancel() {
	new_test_ext().execute_with(|| {
		let call = Box::new(RuntimeCall::Logger(LoggerCall::log_without_filter {
			i: 69,
			weight: Weight::from_parts(10, 0),
		}));
		let call2 = Box::new(RuntimeCall::Logger(LoggerCall::log_without_filter {
			i: 42,
			weight: Weight::from_parts(10, 0),
		}));
		assert_ok!(Scheduler::schedule_named(
			system::RawOrigin::Signed(1).into(),
			[1u8; 32],
			4,
			None,
			127,
			call,
		));
		assert_ok!(Scheduler::schedule(system::RawOrigin::Signed(1).into(), 4, None, 127, call2,));
		run_to_block(3);
		// Scheduled calls are in the agenda.
		assert_eq!(Agenda::<Test>::get(4).len(), 2);
		assert!(logger::log().is_empty());
		assert_noop!(
			Scheduler::cancel_named(system::RawOrigin::Signed(2).into(), [1u8; 32]),
			BadOrigin
		);
		assert_noop!(Scheduler::cancel(system::RawOrigin::Signed(2).into(), 4, 1), BadOrigin);
		assert_noop!(Scheduler::cancel_named(system::RawOrigin::Root.into(), [1u8; 32]), BadOrigin);
		assert_noop!(Scheduler::cancel(system::RawOrigin::Root.into(), 4, 1), BadOrigin);
		run_to_block(5);
		assert_eq!(
			logger::log(),
			vec![
				(system::RawOrigin::Signed(1).into(), 69u32),
				(system::RawOrigin::Signed(1).into(), 42u32)
			]
		);
	});
}

#[test]
fn test_migrate_origin() {
	new_test_ext().execute_with(|| {
		for i in 0..3u64 {
			let k = i.twox_64_concat();
			let old: Vec<Option<Scheduled<[u8; 32], BoundedCallOf<Test>, BoundedVec<u8, ConstU32<512>>, u64, u32, u64>>> = vec![
				Some(Scheduled {
					maybe_id: None,
					priority: i as u8 + 10,
					maybe_call: Some(Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
						i: 96,
						weight: Weight::from_parts(100, 0),
					}))
					.unwrap()),
					maybe_ciphertext: None,
					origin: 3u32,
					maybe_periodic: None,
					_phantom: Default::default(),
				}),
				None,
				Some(Scheduled {
					maybe_id: Some(blake2_256(&b"test"[..])),
					priority: 123,
					origin: 2u32,
					maybe_call: Some(Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
						i: 69,
						weight: Weight::from_parts(10, 0),
					}))
					.unwrap()),
					maybe_ciphertext: None,
					maybe_periodic: Some((456u64, 10)),
					_phantom: Default::default(),
				}),
			];
			frame_support::migration::put_storage_value(b"Scheduler", b"Agenda", &k, old);
		}

		impl Into<OriginCaller> for u32 {
			fn into(self) -> OriginCaller {
				match self {
					3u32 => system::RawOrigin::Root.into(),
					2u32 => system::RawOrigin::None.into(),
					_ => unreachable!("test make no use of it"),
				}
			}
		}

		Scheduler::migrate_origin::<u32>();

		assert_eq_uvec!(
			Agenda::<Test>::iter().map(|x| (x.0, x.1.into_inner())).collect::<Vec<_>>(),
			vec![
				(
					0,
					vec![
						Some(ScheduledOf::<Test> {
							maybe_id: None,
							priority: 10,
							maybe_call: Some(Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
								i: 96,
								weight: Weight::from_parts(100, 0)
							}))
							.unwrap()),
							maybe_ciphertext: None,
							maybe_periodic: None,
							origin: system::RawOrigin::Root.into(),
							_phantom: PhantomData::<u64>::default(),
						}),
						None,
						Some(Scheduled {
							maybe_id: Some(blake2_256(&b"test"[..])),
							priority: 123,
							maybe_call: Some(Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
								i: 69,
								weight: Weight::from_parts(10, 0)
							}))
							.unwrap()),
							maybe_ciphertext: None,
							maybe_periodic: Some((456u64, 10)),
							origin: system::RawOrigin::None.into(),
							_phantom: PhantomData::<u64>::default(),
						}),
					]
				),
				(
					1,
					vec![
						Some(Scheduled {
							maybe_id: None,
							priority: 11,
							maybe_call: Some(Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
								i: 96,
								weight: Weight::from_parts(100, 0)
							}))
							.unwrap()),
							maybe_ciphertext: None,
							maybe_periodic: None,
							origin: system::RawOrigin::Root.into(),
							_phantom: PhantomData::<u64>::default(),
						}),
						None,
						Some(Scheduled {
							maybe_id: Some(blake2_256(&b"test"[..])),
							priority: 123,
							maybe_call: Some(Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
								i: 69,
								weight: Weight::from_parts(10, 0)
							}))
							.unwrap()),
							maybe_ciphertext: None,
							maybe_periodic: Some((456u64, 10)),
							origin: system::RawOrigin::None.into(),
							_phantom: PhantomData::<u64>::default(),
						}),
					]
				),
				(
					2,
					vec![
						Some(Scheduled {
							maybe_id: None,
							priority: 12,
							maybe_call: Some(Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
								i: 96,
								weight: Weight::from_parts(100, 0)
							}))
							.unwrap()),
							maybe_ciphertext: None,
							maybe_periodic: None,
							origin: system::RawOrigin::Root.into(),
							_phantom: PhantomData::<u64>::default(),
						}),
						None,
						Some(Scheduled {
							maybe_id: Some(blake2_256(&b"test"[..])),
							priority: 123,
							maybe_call: Some(Preimage::bound(RuntimeCall::Logger(LoggerCall::log {
								i: 69,
								weight: Weight::from_parts(10, 0)
							}))
							.unwrap()),
							maybe_ciphertext: None,
							maybe_periodic: Some((456u64, 10)),
							origin: system::RawOrigin::None.into(),
							_phantom: PhantomData::<u64>::default(),
						}),
					]
				)
			]
		);
	});
}

#[test]
fn postponed_named_task_cannot_be_rescheduled() {
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(1000, 0) });
		let hash = <Test as frame_system::Config>::Hashing::hash_of(&call);
		let len = call.using_encoded(|x| x.len()) as u32;
		// Important to use here `Bounded::Lookup` to ensure that we request the hash.
		let hashed = Bounded::Lookup { hash, len };
		let name: [u8; 32] = hash.as_ref().try_into().unwrap();

		let address = Scheduler::do_schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			hashed.clone(),
		)
		.unwrap();
		assert!(Preimage::is_requested(&hash));
		assert!(Lookup::<Test>::contains_key(name));

		// Run to a very large block.
		run_to_block(10);
		// It was not executed.
		assert!(logger::log().is_empty());
		assert!(Preimage::is_requested(&hash));
		// Postponing removes the lookup.
		assert!(!Lookup::<Test>::contains_key(name));

		// The agenda still contains the call.
		let agenda = Agenda::<Test>::iter().collect::<Vec<_>>();
		assert_eq!(agenda.len(), 1);
		assert_eq!(
			agenda[0].1,
			vec![Some(Scheduled {
				maybe_id: Some(name),
				priority: 127,
				maybe_call: Some(hashed),
				maybe_ciphertext: None,
				maybe_periodic: None,
				origin: root().into(),
				_phantom: Default::default(),
			})]
		);

		// Finally add the preimage.
		assert_ok!(Preimage::note(call.encode().into()));
		run_to_block(1000);
		// It did not execute.
		assert!(logger::log().is_empty());
		assert!(Preimage::is_requested(&hash));

		// Manually re-schedule the call by name does not work.
		assert_err!(
			Scheduler::do_reschedule_named(name, DispatchTime::At(1001)),
			Error::<Test>::NotFound
		);
		// Manually re-scheduling the call by address errors.
		assert_err!(
			Scheduler::do_reschedule(address, DispatchTime::At(1001)),
			Error::<Test>::Named
		);
	});
}

/// Using the scheduler as `v3::Anon` works.
#[test]
fn scheduler_v3_anon_basic_works() {
	use frame_support::traits::schedule::v3::Anon;
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// Schedule a call.
		let _address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());
		// Executes in block 4.
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		// ... but not again.
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

#[test]
fn scheduler_v3_anon_cancel_works() {
	use frame_support::traits::schedule::v3::Anon;
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();

		// Schedule a call.
		let address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();
		// Cancel the call.
		assert_ok!(<Scheduler as Anon<_, _, _>>::cancel(address));
		// It did not get executed.
		run_to_block(100);
		assert!(logger::log().is_empty());
		// Cannot cancel again.
		assert_err!(<Scheduler as Anon<_, _, _>>::cancel(address), DispatchError::Unavailable);
	});
}

#[test]
fn scheduler_v3_anon_reschedule_works() {
	use frame_support::traits::schedule::v3::Anon;
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// Schedule a call.
		let address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());

		// Cannot re-schedule into the same block.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(4)),
			Error::<Test>::RescheduleNoChange
		);
		// Cannot re-schedule into the past.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(3)),
			Error::<Test>::TargetBlockNumberInPast
		);
		// Re-schedule to block 5.
		assert_ok!(<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(5)));
		// Scheduled for block 5.
		run_to_block(4);
		assert!(logger::log().is_empty());
		run_to_block(5);
		// Does execute in block 5.
		assert_eq!(logger::log(), vec![(root(), 42)]);
		// Cannot re-schedule executed task.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(10)),
			DispatchError::Unavailable
		);
	});
}

#[test]
fn scheduler_v3_anon_next_schedule_time_works() {
	use frame_support::traits::schedule::v3::Anon;
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();

		// Schedule a call.
		let address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());

		// Scheduled for block 4.
		assert_eq!(<Scheduler as Anon<_, _, _>>::next_dispatch_time(address), Ok(4));
		// Block 4 executes it.
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42)]);

		// It has no dispatch time anymore.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::next_dispatch_time(address),
			DispatchError::Unavailable
		);
	});
}

/// Re-scheduling a task changes its next dispatch time.
#[test]
fn scheduler_v3_anon_reschedule_and_next_schedule_time_work() {
	use frame_support::traits::schedule::v3::Anon;
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();

		// Schedule a call.
		let old_address = <Scheduler as Anon<_, _, _>>::schedule(
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());

		// Scheduled for block 4.
		assert_eq!(<Scheduler as Anon<_, _, _>>::next_dispatch_time(old_address), Ok(4));
		// Re-schedule to block 5.
		let address =
			<Scheduler as Anon<_, _, _>>::reschedule(old_address, DispatchTime::At(5)).unwrap();
		assert!(address != old_address);
		// Scheduled for block 5.
		assert_eq!(<Scheduler as Anon<_, _, _>>::next_dispatch_time(address), Ok(5));

		// Block 4 does nothing.
		run_to_block(4);
		assert!(logger::log().is_empty());
		// Block 5 executes it.
		run_to_block(5);
		assert_eq!(logger::log(), vec![(root(), 42)]);
	});
}

#[test]
fn scheduler_v3_anon_schedule_agenda_overflows() {
	use frame_support::traits::schedule::v3::Anon;
	let max: u32 = <Test as Config>::MaxScheduledPerBlock::get();

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();

		// Schedule the maximal number allowed per block.
		for _ in 0..max {
			<Scheduler as Anon<_, _, _>>::schedule(
				DispatchTime::At(4),
				None,
				127,
				root(),
				bound.clone(),
			)
			.unwrap();
		}

		// One more time and it errors.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::schedule(DispatchTime::At(4), None, 127, root(), bound,),
			DispatchError::Exhausted
		);

		run_to_block(4);
		// All scheduled calls are executed.
		assert_eq!(logger::log().len() as u32, max);
	});
}

/// Cancelling and scheduling does not overflow the agenda but fills holes.
#[test]
fn scheduler_v3_anon_cancel_and_schedule_fills_holes() {
	use frame_support::traits::schedule::v3::Anon;
	let max: u32 = <Test as Config>::MaxScheduledPerBlock::get();
	assert!(max > 3, "This test only makes sense for MaxScheduledPerBlock > 3");

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();
		let mut addrs = Vec::<_>::default();

		// Schedule the maximal number allowed per block.
		for _ in 0..max {
			addrs.push(
				<Scheduler as Anon<_, _, _>>::schedule(
					DispatchTime::At(4),
					None,
					127,
					root(),
					bound.clone(),
				)
				.unwrap(),
			);
		}
		// Cancel three of them.
		for addr in addrs.into_iter().take(3) {
			<Scheduler as Anon<_, _, _>>::cancel(addr).unwrap();
		}
		// Schedule three new ones.
		for i in 0..3 {
			let (_block, index) = <Scheduler as Anon<_, _, _>>::schedule(
				DispatchTime::At(4),
				None,
				127,
				root(),
				bound.clone(),
			)
			.unwrap();
			assert_eq!(i, index);
		}

		run_to_block(4);
		// Maximum number of calls are executed.
		assert_eq!(logger::log().len() as u32, max);
	});
}

/// Re-scheduling does not overflow the agenda but fills holes.
#[test]
fn scheduler_v3_anon_reschedule_fills_holes() {
	use frame_support::traits::schedule::v3::Anon;
	let max: u32 = <Test as Config>::MaxScheduledPerBlock::get();
	assert!(max > 3, "pre-condition: This test only makes sense for MaxScheduledPerBlock > 3");

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();
		let mut addrs = Vec::<_>::default();

		// Schedule the maximal number allowed per block.
		for _ in 0..max {
			addrs.push(
				<Scheduler as Anon<_, _, _>>::schedule(
					DispatchTime::At(4),
					None,
					127,
					root(),
					bound.clone(),
				)
				.unwrap(),
			);
		}
		let mut new_addrs = Vec::<_>::default();
		// Reversed last three elements of block 4.
		let last_three = addrs.into_iter().rev().take(3).collect::<Vec<_>>();
		// Re-schedule three of them to block 5.
		for addr in last_three.iter().cloned() {
			new_addrs
				.push(<Scheduler as Anon<_, _, _>>::reschedule(addr, DispatchTime::At(5)).unwrap());
		}
		// Re-scheduling them back into block 3 should result in the same addrs.
		for (old, want) in new_addrs.into_iter().zip(last_three.into_iter().rev()) {
			let new = <Scheduler as Anon<_, _, _>>::reschedule(old, DispatchTime::At(4)).unwrap();
			assert_eq!(new, want);
		}

		run_to_block(4);
		// Maximum number of calls are executed.
		assert_eq!(logger::log().len() as u32, max);
	});
}

/// The scheduler can be used as `v3::Named` trait.
#[test]
fn scheduler_v3_named_basic_works() {
	use frame_support::traits::schedule::v3::Named;

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let name = [1u8; 32];

		// Schedule a call.
		let _address = <Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());
		// Executes in block 4.
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
		// ... but not again.
		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

/// A named task can be cancelled by its name.
#[test]
fn scheduler_v3_named_cancel_named_works() {
	use frame_support::traits::schedule::v3::Named;
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();
		let name = [1u8; 32];

		// Schedule a call.
		<Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();
		// Cancel the call by name.
		assert_ok!(<Scheduler as Named<_, _, _>>::cancel_named(name));
		// It did not get executed.
		run_to_block(100);
		assert!(logger::log().is_empty());
		// Cannot cancel again.
		assert_noop!(<Scheduler as Named<_, _, _>>::cancel_named(name), DispatchError::Unavailable);
	});
}

/// A named task can also be cancelled by its address.
#[test]
fn scheduler_v3_named_cancel_without_name_works() {
	use frame_support::traits::schedule::v3::{Anon, Named};
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();
		let name = [1u8; 32];

		// Schedule a call.
		let address = <Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();
		// Cancel the call by address.
		assert_ok!(<Scheduler as Anon<_, _, _>>::cancel(address));
		// It did not get executed.
		run_to_block(100);
		assert!(logger::log().is_empty());
		// Cannot cancel again.
		assert_err!(<Scheduler as Anon<_, _, _>>::cancel(address), DispatchError::Unavailable);
	});
}

/// A named task can be re-scheduled by its name but not by its address.
#[test]
fn scheduler_v3_named_reschedule_named_works() {
	use frame_support::traits::schedule::v3::{Anon, Named};
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let name = [1u8; 32];

		// Schedule a call.
		let address = <Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());

		// Cannot re-schedule by address.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(10)),
			Error::<Test>::Named,
		);
		// Cannot re-schedule into the same block.
		assert_noop!(
			<Scheduler as Named<_, _, _>>::reschedule_named(name, DispatchTime::At(4)),
			Error::<Test>::RescheduleNoChange
		);
		// Cannot re-schedule into the past.
		assert_noop!(
			<Scheduler as Named<_, _, _>>::reschedule_named(name, DispatchTime::At(3)),
			Error::<Test>::TargetBlockNumberInPast
		);
		// Re-schedule to block 5.
		assert_ok!(<Scheduler as Named<_, _, _>>::reschedule_named(name, DispatchTime::At(5)));
		// Scheduled for block 5.
		run_to_block(4);
		assert!(logger::log().is_empty());
		run_to_block(5);
		// Does execute in block 5.
		assert_eq!(logger::log(), vec![(root(), 42)]);
		// Cannot re-schedule executed task.
		assert_noop!(
			<Scheduler as Named<_, _, _>>::reschedule_named(name, DispatchTime::At(10)),
			DispatchError::Unavailable
		);
		// Also not by address.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::reschedule(address, DispatchTime::At(10)),
			DispatchError::Unavailable
		);
	});
}

#[test]
fn scheduler_v3_named_next_schedule_time_works() {
	use frame_support::traits::schedule::v3::{Anon, Named};
	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let bound = Preimage::bound(call).unwrap();
		let name = [1u8; 32];

		// Schedule a call.
		let address = <Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();

		run_to_block(3);
		// Did not execute till block 3.
		assert!(logger::log().is_empty());

		// Scheduled for block 4.
		assert_eq!(<Scheduler as Named<_, _, _>>::next_dispatch_time(name), Ok(4));
		// Also works by address.
		assert_eq!(<Scheduler as Anon<_, _, _>>::next_dispatch_time(address), Ok(4));
		// Block 4 executes it.
		run_to_block(4);
		assert_eq!(logger::log(), vec![(root(), 42)]);

		// It has no dispatch time anymore.
		assert_noop!(
			<Scheduler as Named<_, _, _>>::next_dispatch_time(name),
			DispatchError::Unavailable
		);
		// Also not by address.
		assert_noop!(
			<Scheduler as Anon<_, _, _>>::next_dispatch_time(address),
			DispatchError::Unavailable
		);
	});
}

#[test]
fn cancel_last_task_removes_agenda() {
	new_test_ext().execute_with(|| {
		let when = 4;
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let address = Scheduler::do_schedule(
			DispatchTime::At(when),
			None,
			127,
			root(),
			Preimage::bound(call.clone()).unwrap(),
		)
		.unwrap();
		let address2 = Scheduler::do_schedule(
			DispatchTime::At(when),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();
		// two tasks at agenda.
		assert!(Agenda::<Test>::get(when).len() == 2);
		assert_ok!(Scheduler::do_cancel(None, address));
		// still two tasks at agenda, `None` and `Some`.
		assert!(Agenda::<Test>::get(when).len() == 2);
		// cancel last task from `when` agenda.
		assert_ok!(Scheduler::do_cancel(None, address2));
		// if all tasks `None`, agenda fully removed.
		assert!(Agenda::<Test>::get(when).len() == 0);
	});
}

#[test]
fn cancel_named_last_task_removes_agenda() {
	new_test_ext().execute_with(|| {
		let when = 4;
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(when),
			None,
			127,
			root(),
			Preimage::bound(call.clone()).unwrap(),
		)
		.unwrap();
		Scheduler::do_schedule_named(
			[2u8; 32],
			DispatchTime::At(when),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();
		// two tasks at agenda.
		assert!(Agenda::<Test>::get(when).len() == 2);
		assert_ok!(Scheduler::do_cancel_named(None, [2u8; 32]));
		// removes trailing `None` and leaves one task.
		assert!(Agenda::<Test>::get(when).len() == 1);
		// cancel last task from `when` agenda.
		assert_ok!(Scheduler::do_cancel_named(None, [1u8; 32]));
		// if all tasks `None`, agenda fully removed.
		assert!(Agenda::<Test>::get(when).len() == 0);
	});
}

#[test]
fn reschedule_last_task_removes_agenda() {
	new_test_ext().execute_with(|| {
		let when = 4;
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let address = Scheduler::do_schedule(
			DispatchTime::At(when),
			None,
			127,
			root(),
			Preimage::bound(call.clone()).unwrap(),
		)
		.unwrap();
		let address2 = Scheduler::do_schedule(
			DispatchTime::At(when),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();
		// two tasks at agenda.
		assert!(Agenda::<Test>::get(when).len() == 2);
		assert_ok!(Scheduler::do_cancel(None, address));
		// still two tasks at agenda, `None` and `Some`.
		assert!(Agenda::<Test>::get(when).len() == 2);
		// reschedule last task from `when` agenda.
		assert_eq!(
			Scheduler::do_reschedule(address2, DispatchTime::At(when + 1)).unwrap(),
			(when + 1, 0)
		);
		// if all tasks `None`, agenda fully removed.
		assert!(Agenda::<Test>::get(when).len() == 0);
	});
}

#[test]
fn reschedule_named_last_task_removes_agenda() {
	new_test_ext().execute_with(|| {
		let when = 4;
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		Scheduler::do_schedule_named(
			[1u8; 32],
			DispatchTime::At(when),
			None,
			127,
			root(),
			Preimage::bound(call.clone()).unwrap(),
		)
		.unwrap();
		Scheduler::do_schedule_named(
			[2u8; 32],
			DispatchTime::At(when),
			None,
			127,
			root(),
			Preimage::bound(call).unwrap(),
		)
		.unwrap();
		// two tasks at agenda.
		assert!(Agenda::<Test>::get(when).len() == 2);
		assert_ok!(Scheduler::do_cancel_named(None, [1u8; 32]));
		// still two tasks at agenda, `None` and `Some`.
		assert!(Agenda::<Test>::get(when).len() == 2);
		// reschedule last task from `when` agenda.
		assert_eq!(
			Scheduler::do_reschedule_named([2u8; 32], DispatchTime::At(when + 1)).unwrap(),
			(when + 1, 0)
		);
		// if all tasks `None`, agenda fully removed.
		assert!(Agenda::<Test>::get(when).len() == 0);
	});
}

/// Ensures that an unvailable call sends an event.
#[test]
fn unavailable_call_is_detected() {
	use frame_support::traits::schedule::v3::Named;

	new_test_ext().execute_with(|| {
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });
		let hash = <Test as frame_system::Config>::Hashing::hash_of(&call);
		let len = call.using_encoded(|x| x.len()) as u32;
		// Important to use here `Bounded::Lookup` to ensure that we request the hash.
		let bound = Bounded::Lookup { hash, len };

		let name = [1u8; 32];

		// Schedule a call.
		let _address = <Scheduler as Named<_, _, _>>::schedule_named(
			name,
			DispatchTime::At(4),
			None,
			127,
			root(),
			bound.clone(),
		)
		.unwrap();

		// Ensure the preimage isn't available
		assert!(!Preimage::have(&bound));

		// Executes in block 4.
		run_to_block(4);

		assert_eq!(
			System::events().last().unwrap().event,
			crate::Event::CallUnavailable { task: (4, 0), id: Some(name) }.into()
		);
	});
}

#[test]
#[docify::export]
fn timelock_basic_scheduling_works() {
	let mut rng = ChaCha20Rng::from_seed([4;32]);

	let ids = vec![
		4u64.to_string().as_bytes().to_vec(), 
	];
	let t = 1;

	let ibe_pp: G2 = G2::generator().into();
	let s = Fr::one();
	let p_pub: G2 = ibe_pp.mul(s).into();

	let ibe_pp_bytes = convert_to_bytes::<G2, 96>(ibe_pp);
	let p_pub_bytes = convert_to_bytes::<G2, 96>(p_pub);

	// Q: how can we mock the decryption trait so that we can d1o whatever?
	// probably don't really need to perform decryption here?
	new_test_ext().execute_with(|| {

		let _ = Etf::set_ibe_params(
			// RuntimeOrigin::root(), 
			&vec![], 
			&ibe_pp_bytes.into(), 
			&p_pub_bytes.into()
		);

		// Call to schedule
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// // then we convert to bytes and encrypt the call
		let ct: etf_crypto_primitives::client::etf_client::AesIbeCt = 
			DefaultEtfClient::<BfIbe>::encrypt(
				ibe_pp_bytes.to_vec(),
				p_pub_bytes.to_vec(),
				&call.encode(),
				ids,
				t,
				&mut rng,
			).unwrap();


		let mut bounded_ct: BoundedVec<u8, ConstU32<512>> = BoundedVec::new();
		ct.aes_ct.ciphertext.iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_ct.try_insert(idx, *i);
		});

		let mut bounded_nonce: BoundedVec<u8, ConstU32<96>> = BoundedVec::new();
		ct.aes_ct.nonce.iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_nonce.try_insert(idx, *i);
		});

		let mut bounded_capsule: BoundedVec<u8, ConstU32<512>> = BoundedVec::new();
		// assumes we only care about a single point in the future
		ct.etf_ct[0].iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_capsule.try_insert(idx, *i);
		});

		let ciphertext = Ciphertext {
			ciphertext: bounded_ct,
			nonce: bounded_nonce,
			capsule: bounded_capsule,
		};

		// Schedule call to be executed at the 4th block
		assert_ok!(Scheduler::do_schedule_sealed(
			DispatchTime::At(4),
			127,
			root(),
			ciphertext,
		));

		// `log` runtime call should not have executed yet
		run_to_block(3);
		assert!(logger::log().is_empty());

		run_to_block(4);
		// `log` runtime call should have executed at block 4
		assert_eq!(logger::log(), vec![(root(), 42u32)]);

		run_to_block(100);
		assert_eq!(logger::log(), vec![(root(), 42u32)]);
	});
}

// TODO: ensure tx fees are properly charged?
#[test]
#[docify::export]
fn timelock_undecryptable_ciphertext_no_execution() {
	let mut rng = ChaCha20Rng::from_seed([4;32]);

	let bad_ids = vec![
		3u64.to_string().as_bytes().to_vec(), 
	];

	let t = 1;

	let ibe_pp: G2 = G2::generator().into();
	let s = Fr::one();
	let p_pub: G2 = ibe_pp.mul(s).into();

	let ibe_pp_bytes = convert_to_bytes::<G2, 96>(ibe_pp);
	let p_pub_bytes = convert_to_bytes::<G2, 96>(p_pub);

	new_test_ext().execute_with(|| {
		let _ = Etf::set_ibe_params(
			// RuntimeOrigin::root(), 
			&vec![], 
			&ibe_pp_bytes.into(), 
			&p_pub_bytes.into()
		);


		// Call to schedule
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// encrypts the ciphertext for the wrong identity
		let ct: etf_crypto_primitives::client::etf_client::AesIbeCt = 
			DefaultEtfClient::<BfIbe>::encrypt(
				ibe_pp_bytes.to_vec(),
				p_pub_bytes.to_vec(),
				&call.encode(),
				bad_ids,
				t,
				&mut rng,
			).unwrap();


		let mut bounded_ct: BoundedVec<u8, ConstU32<512>> = BoundedVec::new();
		ct.aes_ct.ciphertext.iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_ct.try_insert(idx, *i);
		});

		let mut bounded_nonce: BoundedVec<u8, ConstU32<96>> = BoundedVec::new();
		ct.aes_ct.nonce.iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_nonce.try_insert(idx, *i);
		});

		let mut bounded_capsule: BoundedVec<u8, ConstU32<512>> = BoundedVec::new();
		// assumes we only care about a single point in the future
		ct.etf_ct[0].iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_capsule.try_insert(idx, *i);
		});

		let ciphertext = Ciphertext {
			ciphertext: bounded_ct,
			nonce: bounded_nonce,
			capsule: bounded_capsule,
		};

		// Schedule call to be executed at the 4th block
		assert_ok!(Scheduler::do_schedule_sealed(
			DispatchTime::At(4),
			127,
			root(),
			ciphertext,
		));

		// `log` runtime call should not have executed yet
		run_to_block(3);
		assert!(logger::log().is_empty());

		run_to_block(4);
		// `log` runtime call should NOT have executed at block 4
		assert!(logger::log().is_empty());
	});
}

#[test]
#[docify::export]
fn timelock_undecodable_runtime_call_no_execution() {
	let mut rng = ChaCha20Rng::from_seed([4;32]);

	let ids = vec![
		4u64.to_string().as_bytes().to_vec(), 
	];
	let t = 1;

	let ibe_pp: G2 = G2::generator().into();
	let s = Fr::one();
	let p_pub: G2 = ibe_pp.mul(s).into();

	let ibe_pp_bytes = convert_to_bytes::<G2, 96>(ibe_pp);
	let p_pub_bytes = convert_to_bytes::<G2, 96>(p_pub);

	new_test_ext().execute_with(|| {
		let _ = Etf::set_ibe_params(
			// RuntimeOrigin::root(), 
			&vec![], 
			&ibe_pp_bytes.into(), 
			&p_pub_bytes.into()
		);


		// Call to schedule
		// let call =
		// 	RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// encrypts the ciphertext for the wrong identity
		let ct: etf_crypto_primitives::client::etf_client::AesIbeCt = 
			DefaultEtfClient::<BfIbe>::encrypt(
				ibe_pp_bytes.to_vec(),
				p_pub_bytes.to_vec(),
				&b"bad-call-data".encode(),
				ids,
				t,
				&mut rng,
			).unwrap();


		let mut bounded_ct: BoundedVec<u8, ConstU32<512>> = BoundedVec::new();
		ct.aes_ct.ciphertext.iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_ct.try_insert(idx, *i);
		});

		let mut bounded_nonce: BoundedVec<u8, ConstU32<96>> = BoundedVec::new();
		ct.aes_ct.nonce.iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_nonce.try_insert(idx, *i);
		});

		let mut bounded_capsule: BoundedVec<u8, ConstU32<512>> = BoundedVec::new();
		// assumes we only care about a single point in the future
		ct.etf_ct[0].iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_capsule.try_insert(idx, *i);
		});

		let ciphertext = Ciphertext {
			ciphertext: bounded_ct,
			nonce: bounded_nonce,
			capsule: bounded_capsule,
		};

		// Schedule call to be executed at the 4th block
		assert_ok!(Scheduler::do_schedule_sealed(
			DispatchTime::At(4),
			127,
			root(),
			ciphertext,
		));

		// `log` runtime call should not have executed yet
		run_to_block(3);
		assert!(logger::log().is_empty());

		run_to_block(4);
		// `log` runtime call should NOT have executed at block 4
		assert!(logger::log().is_empty());
	});
}

#[test]
#[docify::export]
fn timelock_cancel_works() {
	let mut rng = ChaCha20Rng::from_seed([4;32]);

	let ids = vec![
		4u64.to_string().as_bytes().to_vec(), 
	];
	let t = 1;

	let ibe_pp: G2 = G2::generator().into();
	let s = Fr::one();
	let p_pub: G2 = ibe_pp.mul(s).into();

	let ibe_pp_bytes = convert_to_bytes::<G2, 96>(ibe_pp);
	let p_pub_bytes = convert_to_bytes::<G2, 96>(p_pub);

	// Q: how can we mock the decryption trait so that we can d1o whatever?
	// probably don't really need to perform decryption here?
	new_test_ext().execute_with(|| {

		let _ = Etf::set_ibe_params(
			// RuntimeOrigin::root(), 
			&vec![], 
			&ibe_pp_bytes.into(), 
			&p_pub_bytes.into()
		);

		// Call to schedule
		let call =
			RuntimeCall::Logger(LoggerCall::log { i: 42, weight: Weight::from_parts(10, 0) });

		// // then we convert to bytes and encrypt the call
		let ct: etf_crypto_primitives::client::etf_client::AesIbeCt = 
			DefaultEtfClient::<BfIbe>::encrypt(
				ibe_pp_bytes.to_vec(),
				p_pub_bytes.to_vec(),
				&call.encode(),
				ids,
				t,
				&mut rng,
			).unwrap();


		let mut bounded_ct: BoundedVec<u8, ConstU32<512>> = BoundedVec::new();
		ct.aes_ct.ciphertext.iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_ct.try_insert(idx, *i);
		});

		let mut bounded_nonce: BoundedVec<u8, ConstU32<96>> = BoundedVec::new();
		ct.aes_ct.nonce.iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_nonce.try_insert(idx, *i);
		});

		let mut bounded_capsule: BoundedVec<u8, ConstU32<512>> = BoundedVec::new();
		// assumes we only care about a single point in the future
		ct.etf_ct[0].iter().enumerate().for_each(|(idx, i)| {
			let _= bounded_capsule.try_insert(idx, *i);
		});

		let ciphertext = Ciphertext {
			ciphertext: bounded_ct,
			nonce: bounded_nonce,
			capsule: bounded_capsule,
		};

		// Schedule call to be executed at the 4th block
		assert_ok!(Scheduler::do_schedule_sealed(
			DispatchTime::At(4),
			127,
			root(),
			ciphertext,
		));

		// `log` runtime call should not have executed yet
		run_to_block(3);
		assert!(logger::log().is_empty());


		// now cancel
		assert_ok!(Scheduler::do_cancel(
			None, (4, 0),
		));

		run_to_block(4);
		assert!(logger::log().is_empty());
	});
}