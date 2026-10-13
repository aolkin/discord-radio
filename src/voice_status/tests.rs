use super::*;

#[test]
fn test_status_stack_basic_operations() {
    let mut stack = StatusStack::<String>::new();

    // Test empty stack
    assert!(stack.current().is_none());

    // Test push
    stack.push("First".to_string());
    assert_eq!(stack.current(), Some(&"First".to_string()));

    stack.push("Second".to_string());
    assert_eq!(stack.current(), Some(&"Second".to_string()));

    stack.push("Third".to_string());
    assert_eq!(stack.current(), Some(&"Third".to_string()));
}

#[test]
fn test_status_stack_remove_str() {
    let mut stack = StatusStack::<String>::new();

    stack.push("First".to_string());
    stack.push("Second".to_string());
    stack.push("Third".to_string());

    // Remove from middle
    assert!(stack.remove_str("Second"));
    assert_eq!(stack.current(), Some(&"Third".to_string()));

    // Remove non-existent
    assert!(!stack.remove_str("NonExistent"));
    assert_eq!(stack.current(), Some(&"Third".to_string()));

    // Remove top
    assert!(stack.remove_str("Third"));
    assert_eq!(stack.current(), Some(&"First".to_string()));
}

#[test]
fn test_status_stack_clear() {
    let mut stack = StatusStack::<String>::new();

    stack.push("First".to_string());
    stack.push("Second".to_string());
    stack.push("Third".to_string());

    assert_eq!(stack.current(), Some(&"Third".to_string()));

    stack.clear();
    assert!(stack.current().is_none());
}

#[test]
fn test_status_stack_current_str() {
    let mut stack = StatusStack::<String>::new();

    assert!(stack.current_str().is_none());

    stack.push("Test Status".to_string());
    assert_eq!(stack.current_str(), Some("Test Status"));
}

#[test]
fn test_status_stack_generic_with_integers() {
    let mut stack = StatusStack::<i32>::new();

    assert!(stack.current().is_none());

    stack.push(1);
    stack.push(2);
    stack.push(3);

    assert_eq!(stack.current(), Some(&3));

    stack.clear();
    assert!(stack.current().is_none());
}

#[test]
fn test_activity_entry_creation() {
    let entry = ActivityEntry::new(ActivityType::Playing, "Test Game".to_string());
    assert_eq!(entry.activity_type, ActivityType::Playing);
    assert_eq!(entry.status, "Test Game");
}

#[test]
fn test_activity_entry_equality() {
    let entry1 = ActivityEntry::new(ActivityType::Listening, "Music".to_string());
    let entry2 = ActivityEntry::new(ActivityType::Listening, "Music".to_string());
    let entry3 = ActivityEntry::new(ActivityType::Playing, "Music".to_string());
    let entry4 = ActivityEntry::new(ActivityType::Listening, "Podcast".to_string());

    assert_eq!(entry1, entry2);
    assert_ne!(entry1, entry3);
    assert_ne!(entry1, entry4);
}

#[test]
fn test_activity_stack_with_entries() {
    let mut stack = StatusStack::<ActivityEntry>::new();

    let entry1 = ActivityEntry::new(ActivityType::Playing, "Game 1".to_string());
    let entry2 = ActivityEntry::new(ActivityType::Listening, "Music".to_string());

    stack.push(entry1.clone());
    stack.push(entry2.clone());

    assert_eq!(stack.current(), Some(&entry2));

    // Test remove
    assert!(stack.remove(&entry2));
    assert_eq!(stack.current(), Some(&entry1));
}
