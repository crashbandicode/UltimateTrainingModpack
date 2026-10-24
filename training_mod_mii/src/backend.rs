//! Exact native backend shapes; logical roles are never physical owner indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout { Training, Ldn }
impl Layout {
    pub fn scene(initialized:bool,melee:u8,mode:u32,waiting_active:bool)->Option<Self> {
        match (initialized,melee,mode) {
            (true,0x20,0x04040000)=>Some(Self::Training),
            (true,0x0d,0x02010000) if waiting_active=>Some(Self::Ldn),
            _=>None,
        }
    }
    pub const fn visible(self)->[u32;2] {match self {Self::Training=>[0,1],Self::Ldn=>[0,4]}}
    pub const fn helpers(self)->[u32;2] {match self {Self::Training=>[2,3],Self::Ldn=>[5,6]}}
    pub fn owners(self)->[usize;4] {let v=self.visible();let h=self.helpers();[v[0] as usize,v[1] as usize,h[0] as usize,h[1] as usize]}
    pub fn logical(self,physical:u32)->Option<usize> {self.visible().iter().position(|id|*id==physical)}
    pub fn helper(self,physical:usize)->Option<usize> {self.helpers().iter().position(|id|*id as usize==physical)}
    pub fn selector(self,physical:usize)->bool {physical!=0 && self.owners().contains(&physical)}
    pub fn shape(self,entries:&[usize;8])->bool {
        let ids=self.owners();
        entries.iter().enumerate().all(|(id,p)|(*p!=0)==ids.contains(&id))
            && ids.iter().enumerate().all(|(i,id)|ids[..i].iter().all(|other|entries[*id]!=entries[*other]))
    }
    /// Callback retirement may occur after the mode changed; recover only the exact old map.
    pub fn from_entries(entries:&[usize;8])->Option<Self> {
        [Self::Training,Self::Ldn].into_iter().find(|layout|layout.shape(entries))
    }
    pub fn quantity(self,quantity:Option<u32>)->bool {match self {Self::Training=>quantity==Some(1),Self::Ldn=>quantity.is_none()}}
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
}
