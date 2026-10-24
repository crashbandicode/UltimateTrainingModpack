//! Production-used admission/completion proof, independent of native pointers.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Scene {
    pub epoch: u64,
    pub wrapper: usize,
    pub core: usize,
    pub mode: u32,
    pub melee: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Resource {
    pub address: usize,
    pub state: u32,
    pub kind: u32,
    pub hash: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct View {
    pub scene: Scene,
    pub resource_owner: usize,
    pub task_owner: usize,
    pub task: usize,
    pub task_state: u32,
    pub task_fiber: usize,
    pub disabled: u8,
    pub stop_mask: u32,
    pub state: u32,
    pub active: i32,
    pub incoming: i32,
    pub outgoing: i32,
    pub slots: [usize; 3],
    pub resources: [Option<Resource>; 4],
    pub descriptor: [u8; 0xb0],
    pub waiting_ready: bool,
    pub ui_busy: bool,
    pub roster_busy: bool,
    pub item_ready: bool,
}

impl View {
    pub fn kind(self) -> u32 {
        u32::from_le_bytes([self.descriptor[0], self.descriptor[1], self.descriptor[2], self.descriptor[3]])
    }
    pub fn hash(self) -> u64 {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(&self.descriptor[8..16]);
        u64::from_le_bytes(bytes)
    }
    pub fn native_next(self) -> usize {
        usize::from(self.resources[0].is_some_and(|r| (r.state | 2) == 3))
    }
}

/// Native allocation identities, never dereferenced by pure policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StageLease {
    pub object: usize,
    pub resource: Resource,
}

/// Read only at the observed state6 acknowledgment boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ActivationRead {
    pub native_ready: u8,
    pub ready_bits: u64,
    pub readiness_stop_mask: u32,
    pub target_kind: u32,
    pub target_hash: u64,
    pub target_flag_500: u8,
    pub request_kind: u32,
    pub request_hash: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Commit {
    pub scene: Scene,
    pub resource_owner: usize,
    pub task_owner: usize,
    pub task: usize,
    pub incoming: usize,
    pub outgoing: usize,
    pub target: u32,
    pub hash: u64,
    pub original: StageLease,
    pub activated: Option<StageLease>,
}

fn check(ok: bool, why: &'static str) -> Result<(), &'static str> {
    if ok { Ok(()) } else { Err(why) }
}

pub(super) fn same_owner(v: View, c: Commit) -> bool {
    v.scene == c.scene && v.resource_owner == c.resource_owner
        && v.task_owner == c.task_owner && v.task == c.task
}

pub(super) fn current_request(queued: Scene, current: Scene, original: usize, active: usize) -> bool {
    queued == current && original != 0 && original == active
}

/// The native scene driver (14a569c..56b4), independently repeated at
/// 24b9a94..9aac, explicitly acknowledges state6 only when native dc is ready.
/// It does not force readiness or enqueue another load. The manager then owns
/// states7..12, including observer acknowledgments and outgoing destruction.
pub(super) fn acknowledge(v: View, c: Commit, a: ActivationRead) -> Result<Option<Commit>, &'static str> {
    check(same_owner(v,c), "activation owner changed")?;
    check(c.activated.is_none(), "activation already acknowledged")?;
    check(v.state==6 && v.active==c.outgoing as i32
        && v.incoming==c.incoming as i32 && v.outgoing==c.outgoing as i32,
        "activation phase or pair changed")?;
    check(v.task_state==0 && v.task_fiber==0, "activation task not finalized")?;
    check(v.slots[c.outgoing]==c.original.object
        && v.resources[c.outgoing]==Some(c.original.resource)
        && v.kind()==c.original.resource.kind && v.hash()==c.original.resource.hash,
        "activation original allocation changed")?;
    check(v.slots.iter().enumerate().all(|(i,p)|i==c.incoming || i==c.outgoing || *p==0)
        && v.resources.iter().enumerate().all(|(i,r)|i==c.incoming || i==c.outgoing || r.is_none()),
        "activation conflicting native slot")?;
    let resource=v.resources[c.incoming].ok_or("activation target resource absent")?;
    check(v.slots[c.incoming]!=0 && v.slots[c.incoming]!=c.original.object
        && resource.address!=c.original.resource.address && resource.state==1
        && resource.kind==c.target && resource.hash==c.hash
        && a.target_kind==c.target && a.target_hash==c.hash && a.target_flag_500==1
        && a.request_kind==c.target && a.request_hash==c.hash,
        "activation target not constructed or changed")?;
    check(a.native_ready<=1, "activation native readiness inconsistent")?;
    // Suspension and a stale per-frame readiness pair are waits, not lost
    // ownership. Check all immutable authority above before deferring again.
    if v.disabled!=0 || v.stop_mask!=0 || a.readiness_stop_mask!=0
        || v.ui_busy || v.roster_busy
        || (v.scene.mode==0x02010000 && !v.waiting_ready)
        || a.native_ready==0 || a.ready_bits!=3 {
        return Ok(None);
    }
    let mut result=c;
    result.activated=Some(StageLease { object:v.slots[c.incoming],resource });
    Ok(Some(result))
}

/// State0 alone is insufficient: exact retained indices, active descriptor,
/// outgoing release, task finalization and no cache/extra slot are required.
pub(super) fn complete(v: View, c: Commit) -> bool {
    same_owner(v, c) && v.state == 0 && v.task_state == 0 && v.task_fiber == 0
        && v.active == c.incoming as i32 && v.incoming == c.incoming as i32
        && v.outgoing == c.outgoing as i32 && v.kind() == c.target && v.hash() == c.hash
        && c.activated.is_some_and(|a| v.slots[c.incoming]==a.object
            && v.resources[c.incoming]==Some(a.resource)
            && a.resource.state==1 && a.resource.kind==c.target && a.resource.hash==c.hash)
        && v.slots.iter().enumerate().all(|(i, p)| i == c.incoming || *p == 0)
        && v.resources.iter().enumerate().all(|(i, r)| i == c.incoming || r.is_none())
}

pub(super) fn admit(v: View, settled: Option<Commit>) -> Result<usize, &'static str> {
    check((v.scene.mode == 0x04040000 && v.scene.melee == 0x20)
        || (v.scene.mode == 0x02010000 && v.scene.melee == 0x0d && v.waiting_ready), "not owned practice scene")?;
    check(v.disabled == 0 && v.stop_mask == 0 && !v.ui_busy && !v.roster_busy, "practice suspended")?;
    check(v.state == 0 && v.task_state == 0 && v.task_fiber == 0, "native transition or task busy")?;
    check(v.item_ready, "native item gate closed")?;
    check((0..3).contains(&v.active), "invalid active slot")?;
    let active = v.active as usize;
    check(v.slots[active] != 0, "missing active stage")?;
    // Both state0 and state1 cache objects are ownership, never spare capacity.
    check(v.resources[3].is_none(), "pending match cache occupied")?;
    check(v.resources[active].is_some_and(|r| r.state == 1 && r.kind == v.kind() && r.hash == v.hash()), "active resource mismatch")?;
    check(v.slots.iter().enumerate().all(|(i, p)| i == active || *p == 0)
        && v.resources.iter().enumerate().all(|(i, r)| i == active || r.is_none()), "other native slot owned")?;
    check((v.incoming == -1 && v.outgoing == -1)
        || settled.is_some_and(|c| complete(v, c)), "ambiguous incoming/outgoing ownership")?;
    let next = v.native_next();
    check(next != active && v.slots[next] == 0 && v.resources[next].is_none(), "native selected slot not free")?;
    Ok(next)
}

pub(super) fn plan(v: View, settled: Option<Commit>, target: u32, hash: u64) -> Result<(Commit, [u8; 0xb0]), &'static str> {
    let next = admit(v, settled)?;
    check(target != v.kind(), "stage already active")?;
    check(hash != 0 && hash <= 0xffffffffff, "invalid stage hash")?;
    let mut descriptor = v.descriptor;
    descriptor[..4].copy_from_slice(&target.to_le_bytes());
    descriptor[8..16].copy_from_slice(&hash.to_le_bytes());
    Ok((Commit { scene: v.scene, resource_owner: v.resource_owner,
        task_owner: v.task_owner, task: v.task, incoming: next, outgoing: v.active as usize,
        target, hash, original:StageLease { object:v.slots[v.active as usize],
            resource:v.resources[v.active as usize].ok_or("active resource absent")? }, activated:None }, descriptor))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn waiting() -> View {
        let mut descriptor = [0x5a; 0xb0];
        descriptor[..4].copy_from_slice(&311u32.to_le_bytes());
        descriptor[8..16].copy_from_slice(&0x165a48f367u64.to_le_bytes());
        View { scene: Scene { epoch: 7, wrapper: 8, core: 16, mode: 0x02010000, melee: 0xd },
            resource_owner: 24, task_owner: 32, task: 40, task_state: 0, task_fiber: 0,
            disabled: 0, stop_mask: 0, state: 0, active: 2, incoming: -1, outgoing: -1,
            slots: [0, 0, 48], resources: [None, None, Some(Resource { address: 56,
                state: 1, kind: 311, hash: 0x165a48f367 }), None], descriptor,
            waiting_ready: true, ui_busy: false, roster_busy: false, item_ready: true }
    }
    #[test]
    fn actual_waiting_uses_native_zero_and_preserves_full_descriptor() {
        let v = waiting();
        let (c, d) = plan(v, None, 0, 0x123456789a).unwrap();
        assert_eq!((c.incoming, c.outgoing), (0, 2)); // active XOR1 would be invalid3.
        assert_eq!(&d[4..8], &v.descriptor[4..8]);
        assert_eq!(&d[16..], &v.descriptor[16..]);
        assert_eq!(&d[..4], &0u32.to_le_bytes());
        assert_eq!(&d[8..16], &0x123456789au64.to_le_bytes());
        let mut training = v; training.scene.mode=0x04040000; training.scene.melee=0x20;
        training.active=0; training.slots=[48,0,0]; training.resources.swap(0,2);
        assert_eq!(admit(training,None),Ok(1));
    }
    #[test]
    fn real_pending_match_cache_blocks_both_loading_and_ready() {
        for state in [0, 1] {
            let mut v = waiting();
            v.resources[3]=Some(Resource { address: 64, state, kind: 44, hash: 0x1cb58f24d5 });
            assert_eq!(admit(v,None),Err("pending match cache occupied"));
        }
    }
    #[test]
    fn task_active_slot_and_unowned_indices_are_not_capacity() {
        let mut v=waiting(); v.task_state=5;
        assert_eq!(admit(v,None),Err("native transition or task busy"));
        v=waiting(); v.task_fiber=64;
        assert_eq!(admit(v,None),Err("native transition or task busy"));
        v=waiting(); v.incoming=0;
        assert_eq!(admit(v,None),Err("ambiguous incoming/outgoing ownership"));
        v=waiting(); v.slots[0]=64;
        assert_eq!(admit(v,None),Err("other native slot owned"));
        v=waiting(); v.resources[2].as_mut().unwrap().hash=0;
        assert_eq!(admit(v,None),Err("active resource mismatch"));
    }
    #[test]
    fn only_exact_native_completion_permits_a_second_request() {
        let v=waiting(); let(c,_)=plan(v,None,0,0x123456789a).unwrap();
        let (loaded,ready)=loaded(v,c);
        let c=acknowledge(loaded,c,ready).unwrap().unwrap();
        let mut done=v; done.active=0; done.incoming=0; done.outgoing=2;
        done.slots=[72,0,0]; done.resources=[Some(Resource { address:80,state:1,kind:0,hash:c.hash }),None,None,None];
        done.descriptor[..4].copy_from_slice(&0u32.to_le_bytes());
        done.descriptor[8..16].copy_from_slice(&c.hash.to_le_bytes());
        assert!(complete(done,c));
        assert!(admit(done,None).is_err());
        assert_eq!(admit(done,Some(c)),Ok(1));
        for altered in 0..6 {
            let mut bad=done;
            match altered { 0=>bad.scene.epoch+=1, 1=>bad.task_state=5,
                2=>bad.slots[2]=48, 3=>bad.task_owner+=8,
                4=>bad.slots[0]+=8, _=>bad.resources[0].as_mut().unwrap().address+=8 }
            assert!(!complete(bad,c));
            assert!(admit(bad,Some(c)).is_err());
        }
    }
    fn loaded(mut v:View,c:Commit)->(View,ActivationRead) {
        v.state=6; v.incoming=c.incoming as i32; v.outgoing=c.outgoing as i32;
        v.slots[c.incoming]=72;
        v.resources[c.incoming]=Some(Resource { address:80,state:1,kind:c.target,hash:c.hash });
        (v,ActivationRead { native_ready:1,ready_bits:3,readiness_stop_mask:0,
            target_kind:c.target,target_hash:c.hash,target_flag_500:1,
            request_kind:c.target,request_hash:c.hash })
    }
    #[test]
    fn observed_state6_requires_native_readiness_and_exact_allocation_lease_once() {
        let v=waiting();let(c,_)=plan(v,None,0,0x123456789a).unwrap();
        let (loaded,ready)=loaded(v,c);
        assert!(!complete(loaded,c));
        let mut not_ready=ready;not_ready.native_ready=0;not_ready.ready_bits=1;
        assert_eq!(acknowledge(loaded,c,not_ready),Ok(None));
        let ack=acknowledge(loaded,c,ready).unwrap().unwrap();
        assert_eq!(ack.original,c.original);
        assert_eq!(ack.activated.unwrap().object,72);
        assert_eq!(acknowledge(loaded,ack,ready),Err("activation already acknowledged"));
        let mut changed=loaded;changed.slots[2]+=8;
        assert_eq!(acknowledge(changed,c,ready),Err("activation original allocation changed"));
        changed=loaded;changed.resources[2].as_mut().unwrap().address+=8;
        assert_eq!(acknowledge(changed,c,ready),Err("activation original allocation changed"));
        changed=loaded;changed.resources[3]=Some(Resource { address:88,state:0,kind:44,hash:99 });
        assert_eq!(acknowledge(changed,c,ready),Err("activation conflicting native slot"));
        changed=loaded;changed.scene.epoch+=1;
        assert_eq!(acknowledge(changed,c,ready),Err("activation owner changed"));
        changed=loaded;changed.task_fiber=96;
        assert_eq!(acknowledge(changed,c,ready),Err("activation task not finalized"));
        let mut wrong=ready;wrong.target_hash+=1;
        assert_eq!(acknowledge(loaded,c,wrong),Err("activation target not constructed or changed"));
        wrong=ready;wrong.native_ready=2;
        assert_eq!(acknowledge(loaded,c,wrong),Err("activation native readiness inconsistent"));
    }
    #[test]
    fn state6_transient_wait_clears_then_acknowledges_the_same_lease() {
        let v=waiting();let(c,_)=plan(v,None,0,0x123456789a).unwrap();
        let (loaded,ready)=loaded(v,c);
        let expected=acknowledge(loaded,c,ready).unwrap().unwrap();
        for temporary in 0..7 {
            let mut suspended=loaded;let mut not_ready=ready;
            match temporary {
                0=>suspended.disabled=1,
                1=>suspended.stop_mask=1,
                2=>not_ready.readiness_stop_mask=1,
                3=>suspended.ui_busy=true,
                4=>suspended.roster_busy=true,
                5=>suspended.waiting_ready=false,
                _=>not_ready.ready_bits=1, // dc1 with a stale per-frame e0 pair.
            }
            assert_eq!(acknowledge(suspended,c,not_ready),Ok(None));
            assert_eq!(acknowledge(loaded,c,ready),Ok(Some(expected)));
            assert_eq!(acknowledge(loaded,expected,ready),Err("activation already acknowledged"));
        }
    }
    #[test]
    fn state6_transient_wait_does_not_mask_lost_authority() {
        let v=waiting();let(c,_)=plan(v,None,0,0x123456789a).unwrap();
        let (mut suspended,mut stale)=loaded(v,c);
        suspended.ui_busy=true;stale.ready_bits=1;
        let mut changed=suspended;changed.scene.epoch+=1;
        assert_eq!(acknowledge(changed,c,stale),Err("activation owner changed"));
        changed=suspended;changed.incoming=1;
        assert_eq!(acknowledge(changed,c,stale),Err("activation phase or pair changed"));
        changed=suspended;changed.resources[c.outgoing].as_mut().unwrap().address+=8;
        assert_eq!(acknowledge(changed,c,stale),Err("activation original allocation changed"));
        let mut changed_read=stale;changed_read.target_hash+=1;
        assert_eq!(acknowledge(suspended,c,changed_read),Err("activation target not constructed or changed"));
    }
    #[test]
    fn departure_cancels_even_reused_addresses_before_publication() {
        let old=waiting().scene;
        assert!(current_request(old,old,48,48));
        let mut returned=old; returned.epoch+=1;
        assert!(!current_request(old,returned,48,48));
        assert!(!current_request(old,old,48,72));
    }
}
