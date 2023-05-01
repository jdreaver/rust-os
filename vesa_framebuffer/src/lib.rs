#![cfg_attr(not(test), no_std)]

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(1, 2), 3);
    }
}
