@0x8cb5e111d76d909c;

enum Role { reader @0; author @1; admin @2; }
enum PostState { draft @0; published @1; hidden @2; }
enum Source { web @0; rss @1; sitemap @2; api @3; }
enum DocumentKind { page @0; feed @1; image @2; pdf @3; }
enum DocumentState { pending @0; fetched @1; failed @2; gone @3; }

struct User {
  idHi @0 :UInt64;
  idLo @1 :UInt64;
  email @2 :Text;
  role @3 :Role;
  created @4 :Int64;
  lastSeen :union {
    unset @5 :Void;
    at @6 :Int64;
  }
}

struct Image {
  key @0 :Text;
  width @1 :UInt16;
  height @2 :UInt16;
}

struct Post {
  idHi @0 :UInt64;
  idLo @1 :UInt64;
  authorHi @2 :UInt64;
  authorLo @3 :UInt64;
  slug @4 :Text;
  title @5 :Text;
  state @6 :PostState;
  created @7 :Int64;
  changed @8 :Int64;
  cover @9 :Image;
  tags @10 :List(Text);
}

struct Etag {
  value @0 :Text;
  seenAt @1 :Int64;
}

struct Document {
  idHi @0 :UInt64;
  idLo @1 :UInt64;
  source @2 :Source;
  kind @3 :DocumentKind;
  key @4 :Text;
  parent :union {
    unset @5 :Void;
    uuid :group {
      hi @6 :UInt64;
      lo @7 :UInt64;
    }
  }
  seq :union {
    unset @8 :Void;
    value @9 :UInt64;
  }
  state @10 :DocumentState;
  fetchedAt :union {
    unset @11 :Void;
    at @12 :Int64;
  }
  changedAt :union {
    unset @13 :Void;
    at @14 :Int64;
  }
  etags @15 :List(Etag);
}

struct Posts {
  items @0 :List(Post);
}
