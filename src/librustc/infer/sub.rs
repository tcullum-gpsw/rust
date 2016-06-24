// Copyright 2012 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use super::combine::CombineFields;
use super::SubregionOrigin;
use super::type_variable::{SubtypeOf, SupertypeOf};

use ty::{self, Ty, TyCtxt};
use ty::TyVar;
use ty::relate::{Cause, Relate, RelateResult, TypeRelation};
use traits::PredicateObligations;
use std::mem;

/// Ensures `a` is made a subtype of `b`. Returns `a` on success.
pub struct Sub<'a, 'gcx: 'a+'tcx, 'tcx: 'a> {
    fields: CombineFields<'a, 'gcx, 'tcx>,
}

impl<'a, 'gcx, 'tcx> Sub<'a, 'gcx, 'tcx> {
    pub fn new(f: CombineFields<'a, 'gcx, 'tcx>) -> Sub<'a, 'gcx, 'tcx> {
        Sub { fields: f }
    }

    pub fn obligations(self) -> PredicateObligations<'tcx> {
        self.fields.obligations
    }
}

impl<'a, 'gcx, 'tcx> TypeRelation<'a, 'gcx, 'tcx> for Sub<'a, 'gcx, 'tcx> {
    fn tag(&self) -> &'static str { "Sub" }
    fn tcx(&self) -> TyCtxt<'a, 'gcx, 'tcx> { self.fields.infcx.tcx }
    fn a_is_expected(&self) -> bool { self.fields.a_is_expected }

    fn with_cause<F,R>(&mut self, cause: Cause, f: F) -> R
        where F: FnOnce(&mut Self) -> R
    {
        debug!("sub with_cause={:?}", cause);
        let old_cause = mem::replace(&mut self.fields.cause, Some(cause));
        let r = f(self);
        debug!("sub old_cause={:?}", old_cause);
        self.fields.cause = old_cause;
        r
    }

    fn relate_with_variance<T: Relate<'tcx>>(&mut self,
                                             variance: ty::Variance,
                                             a: &T,
                                             b: &T)
                                             -> RelateResult<'tcx, T>
    {
        match variance {
            ty::Invariant => self.fields.equate().relate(a, b),
            ty::Covariant => self.relate(a, b),
            ty::Bivariant => self.fields.bivariate().relate(a, b),
            ty::Contravariant => self.fields.switch_expected().sub().relate(b, a),
        }
    }

    fn tys(&mut self, a: Ty<'tcx>, b: Ty<'tcx>) -> RelateResult<'tcx, Ty<'tcx>> {
        debug!("{}.tys({:?}, {:?})", self.tag(), a, b);

        if a == b { return Ok(a); }

        let infcx = self.fields.infcx;
        let a = infcx.type_variables.borrow_mut().replace_if_possible(a);
        let b = infcx.type_variables.borrow_mut().replace_if_possible(b);
        match (&a.sty, &b.sty) {
            (&ty::TyInfer(TyVar(a_id)), &ty::TyInfer(TyVar(b_id))) => {
                infcx.type_variables
                    .borrow_mut()
                    .relate_vars(a_id, SubtypeOf, b_id);
                Ok(a)
            }
            (&ty::TyInfer(TyVar(a_id)), _) => {
                self.fields
                    .switch_expected()
                    .instantiate(b, SupertypeOf, a_id)?;
                Ok(a)
            }
            (_, &ty::TyInfer(TyVar(b_id))) => {
                self.fields.instantiate(a, SubtypeOf, b_id)?;
                Ok(a)
            }

            (&ty::TyError, _) | (_, &ty::TyError) => {
                infcx.set_tainted_by_errors();
                Ok(self.tcx().types.err)
            }

            _ => {
                self.fields.infcx.super_combine_tys(self, a, b)?;
                Ok(a)
            }
        }
    }

    fn regions(&mut self, a: ty::Region, b: ty::Region) -> RelateResult<'tcx, ty::Region> {
        debug!("{}.regions({:?}, {:?}) self.cause={:?}",
               self.tag(), a, b, self.fields.cause);
        // FIXME -- we have more fine-grained information available
        // from the "cause" field, we could perhaps give more tailored
        // error messages.
        let origin = SubregionOrigin::Subtype(self.fields.trace.clone());
        self.fields.infcx.region_vars.make_subregion(origin, a, b);
        Ok(a)
    }

    fn binders<T>(&mut self, a: &ty::Binder<T>, b: &ty::Binder<T>)
                  -> RelateResult<'tcx, ty::Binder<T>>
        where T: Relate<'tcx>
    {
        self.fields.higher_ranked_sub(a, b)
    }
}