//! Exact native backend shapes; logical roles are never physical owner indices.
//!
//! Local-wireless and Quickplay waiting share one PracticeScene setup (13.0.5
//! `0x1790020`, reached identically from the general controller `0x2310570` and
//! the online controller `0x237e970`): one primary row/entry 0 and CPU rows/entries
//! 4/5/6, with no mode branch around that construction. They stay distinct variants
//! so a mode word is never inferred from a shape and every live-shape gate still runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout { Training, Ldn, Quickplay }
impl Layout {
    /// Quickplay waiting mode words seen by the shared setup (`0x070101xx`). Arenas
    /// (`0x07010300`) and `0x07010400` stay outside until qualified separately.
    pub const fn quickplay_waiting_mode(mode:u32)->bool {mode & 0x7fffff00 == 0x07010100}
    pub fn scene(initialized:bool,melee:u8,mode:u32,waiting_active:bool)->Option<Self> {
        match (initialized,melee,mode) {
            (true,0x20,0x04040000)=>Some(Self::Training),
            (true,0x0d,0x02010000) if waiting_active=>Some(Self::Ldn),
            (true,0x0d,mode) if waiting_active && Self::quickplay_waiting_mode(mode)=>Some(Self::Quickplay),
            _=>None,
        }
    }
    /// The shared waiting shape: visible 0/4, helpers 5/6, native selection indices.
    pub const fn waiting(self)->bool {!matches!(self,Self::Training)}
    pub const fn visible(self)->[u32;2] {match self {Self::Training=>[0,1],Self::Ldn|Self::Quickplay=>[0,4]}}
    pub const fn helpers(self)->[u32;2] {match self {Self::Training=>[2,3],Self::Ldn|Self::Quickplay=>[5,6]}}
    pub fn owners(self)->[usize;4] {let v=self.visible();let h=self.helpers();[v[0] as usize,v[1] as usize,h[0] as usize,h[1] as usize]}
    pub fn logical(self,physical:u32)->Option<usize> {self.visible().iter().position(|id|*id==physical)}
    pub fn helper(self,physical:usize)->Option<usize> {self.helpers().iter().position(|id|*id as usize==physical)}
    pub fn selector(self,physical:usize)->bool {physical!=0 && self.owners().contains(&physical)}
    pub fn shape(self,entries:&[usize;8])->bool {
        let ids=self.owners();
        entries.iter().enumerate().all(|(id,p)|(*p!=0)==ids.contains(&id))
            && ids.iter().enumerate().all(|(i,id)|ids[..i].iter().all(|other|entries[*id]!=entries[*other]))
    }
    pub fn quantity(self,quantity:Option<u32>)->bool {match self {Self::Training=>quantity==Some(1),Self::Ldn|Self::Quickplay=>quantity.is_none()}}
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn exact_backend_shape_and_logical_cpu() {
        let map=[0x1000,0,0,0,0x2000,0x3000,0x4000,0];
        assert_eq!(Layout::scene(true,13,0x02010000,true),Some(Layout::Ldn));
        for (melee,mode) in [(1,0x02010000),(13,0x07010000),(32,0x02010000)] {
            assert_eq!(Layout::scene(true,melee,mode,true),None);
        }
        assert!(Layout::Ldn.shape(&map));assert!(!Layout::Training.shape(&map));
        assert_eq!(Layout::Ldn.logical(4),Some(1));assert_eq!(Layout::Ldn.logical(1),None);
        assert_eq!(Layout::Ldn.helper(5),Some(0));assert_eq!(Layout::Ldn.helper(6),Some(1));
        assert!(Layout::Ldn.quantity(None));assert!(!Layout::Ldn.quantity(Some(1)));
        assert!(Layout::Training.quantity(Some(1)));assert!(!Layout::Training.quantity(None));
        let mut alias=map;alias[6]=map[5];assert!(!Layout::Ldn.shape(&alias));
    }
    #[test] fn quickplay_waiting_shares_the_shape_but_keeps_its_own_mode_family() {
        for mode in [0x07010100,0x07010101,0x07010102] {
            assert_eq!(Layout::scene(true,0x0d,mode,true),Some(Layout::Quickplay));
            assert_eq!(Layout::scene(true,0x0d,mode,false),None);
            assert_eq!(Layout::scene(false,0x0d,mode,true),None);
        }
        // Arenas, the 0x07010400 family, the online match melee and other families stay out.
        for (melee,mode) in [(0x0d,0x07010300),(0x0d,0x07010400),(0x0e,0x07010101),(0x01,0x07010101),(0x0d,0x08010100),(0x20,0x07010101)] {
            assert_eq!(Layout::scene(true,melee,mode,true),None);
        }
        assert_ne!(Layout::Quickplay,Layout::Ldn);
        assert!(Layout::Quickplay.waiting() && Layout::Ldn.waiting() && !Layout::Training.waiting());
        let map=[0x1000,0,0,0,0x2000,0x3000,0x4000,0];
        assert!(Layout::Quickplay.shape(&map));
        assert_eq!(Layout::Quickplay.visible(),Layout::Ldn.visible());
        assert_eq!(Layout::Quickplay.helpers(),Layout::Ldn.helpers());
        assert_eq!(Layout::Quickplay.owners(),Layout::Ldn.owners());
        assert_eq!(Layout::Quickplay.logical(4),Some(1));assert_eq!(Layout::Quickplay.logical(1),None);
        assert_eq!(Layout::Quickplay.helper(5),Some(0));assert_eq!(Layout::Quickplay.helper(6),Some(1));
        assert!(Layout::Quickplay.selector(5) && !Layout::Quickplay.selector(0) && !Layout::Quickplay.selector(2));
        assert!(Layout::Quickplay.quantity(None) && !Layout::Quickplay.quantity(Some(1)));
        let mut alias=map;alias[6]=map[5];assert!(!Layout::Quickplay.shape(&alias));
    }
}
