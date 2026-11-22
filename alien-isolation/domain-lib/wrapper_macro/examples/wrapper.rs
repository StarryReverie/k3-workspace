use wrapper_macro::core_lib_impl;

#[core_lib_impl]
trait A {
    fn get_x(&self, t: usize) -> usize;
}

pub struct X;

impl A for X {
    fn get_x(&self, t: usize) -> usize {
        8
    }
}

fn main() {}
