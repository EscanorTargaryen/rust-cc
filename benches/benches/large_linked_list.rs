use std::cell::RefCell;

use rust_cc::*;

pub fn large_linked_list(size: usize) -> Vec<usize> {
    let mut res = Vec::new();
    for _ in 0..30 {
        let mut list = List::new();
        for _ in 0..size {
            list.add();
        }
        res.push(list.len());
    }
    collect_cycles();
    res
}

struct List {
    head: Cc<Node>,
}

impl List {
    fn new() -> List {
        List {
            head: Cc::new(Node::Nil),
        }
    }

    fn add(&mut self) {
        let cons = Cc::new(Node::Cons {
            next: self.head.clone(),
            previous: RefCell::new(None),
        });
        if let Node::Cons{ previous, .. } = &*self.head {
            *previous.borrow_mut() = Some(cons.clone());
        }
        self.head = cons;
    }

    fn len(&self) -> usize {
        self.head.len()
    }
}

enum Node {
    Cons { next: Cc<Node>, previous: RefCell<Option<Cc<Node>>> },
    Nil,
}

impl Node {
    fn len(&self) -> usize {
        match self {
            Self::Cons { next, .. } => {
                next.len() + 1
            },
            _ => 0,
        }
    }
}

unsafe impl Trace for Node {
    fn trace(&self, ctx: &mut Context<'_>) {
        match self {
            Self::Cons { next, previous } => {
                next.trace(ctx);
                previous.trace(ctx);
            },
            Self::Nil => {},
        }
    }
}

impl Finalize for Node {}
