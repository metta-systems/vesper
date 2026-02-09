use core::cmp::Ordering;

// In-place quicksort from https://github.com/jlkiri/rust-sorting-algorithms/blob/master/src/quick.rs
fn partition<T: Copy>(
    array: &mut [T],
    l: isize,
    h: isize,
    compare: fn(&T, &T) -> Ordering,
) -> isize {
    let pivot = array[h as usize];
    let mut i = l - 1; // Index of the smaller element

    for j in l..h {
        if compare(&array[j as usize], &pivot) != Ordering::Greater {
            i = i + 1;
            array.swap(i as usize, j as usize);
        }
    }

    array.swap((i + 1) as usize, h as usize);

    i + 1
}

fn quick_sort_partition<T: Copy>(
    array: &mut [T],
    start: isize,
    end: isize,
    compare: fn(&T, &T) -> Ordering,
) {
    if start < end && end - start >= 1 {
        let pivot = partition(array, start as isize, end as isize, compare);
        quick_sort_partition(array, start, pivot - 1, compare);
        quick_sort_partition(array, pivot + 1, end, compare);
    }
}

pub fn sort<T: Copy>(array: &mut [T], compare: fn(&T, &T) -> Ordering) {
    let start = 0;
    let end = array.len() - 1;
    quick_sort_partition(array, start, end as isize, compare);
}
