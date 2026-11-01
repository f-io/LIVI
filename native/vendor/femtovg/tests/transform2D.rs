use femtovg::Transform2D;

#[test]
fn test_multiplication() {
    let a = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let b = Transform2D::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);
    let expected = Transform2D::new(25.0, 28.0, 57.0, 64.0, 100.0, 112.0);

    let result = a * b;
    assert_eq!(result, expected);
}

#[test]
fn test_multiplication_assignment() {
    let mut a = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let b = Transform2D::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);
    let expected = Transform2D::new(25.0, 28.0, 57.0, 64.0, 100.0, 112.0);

    a *= b;
    assert_eq!(a, expected);
}

#[test]
fn test_premultiply() {
    let a = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let b = Transform2D::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);

    let mut premultiplied = a;
    premultiplied.premultiply(&b);

    assert_eq!(premultiplied, b * a);
}

#[test]
fn test_division() {
    let a = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let b = Transform2D::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);
    let expected = Transform2D::new(-2.0, 3.0, -3.0, 4.0, -3.0, 3.0);

    let result = b / a;
    assert_eq!(result, expected);
}

#[test]
fn test_division_assignment() {
    let a = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let mut b = Transform2D::new(7.0, 8.0, 9.0, 10.0, 11.0, 12.0);
    let expected = Transform2D::new(-2.0, 3.0, -3.0, 4.0, -3.0, 3.0);

    b /= a;
    assert_eq!(b, expected);
}

#[test]
fn test_translation() {
    let transform = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let (tx, ty) = (2.0, 5.0);

    let mut translated = transform;
    translated.translate(tx, ty);

    assert_eq!(translated, transform * Transform2D::translation(tx, ty))
}

#[test]
fn test_rotation() {
    let transform = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let a = std::f32::consts::TAU * 0.75;

    let mut scaled = transform;
    scaled.rotate(a);

    assert_eq!(scaled, transform * Transform2D::rotation(a))
}

#[test]
fn test_scaling() {
    let transform = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let (sx, sy) = (2.0, 0.5);

    let mut scaled = transform;
    scaled.scale(sx, sy);

    assert_eq!(scaled, transform * Transform2D::scaling(sx, sy))
}
