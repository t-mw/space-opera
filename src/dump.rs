// see https://www.reddit.com/r/rust/comments/23o5ex/better_print_macros/
#[allow(unused_macros)]
#[cfg(debug_assertions)]
macro_rules! dump {
  ($($a:expr),*) => ({
    let mut txt = format!("{}:{}:", file!(), line!());
    $({txt += &format!("\t{}={:?};", stringify!($a), $a)});*;
    println!("DEBUG: {}", txt);
  })
}

#[cfg(not(debug_assertions))]
macro_rules! dump {
    ($($a:expr),*) => {};
}
