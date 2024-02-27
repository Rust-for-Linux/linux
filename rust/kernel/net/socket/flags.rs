// SPDX-License-Identifier: GPL-2.0

//! Socket-related flags and utilities.
use crate::bindings;
use core::fmt::Debug;
use core::ops::{BitOr, BitOrAssign};

/// Generic socket flag trait.
///
/// This trait represents any kind of flag with "bitmask" values (i.e. 0x1, 0x2, 0x4, 0x8, etc.)
pub trait Flag:
    Into<isize> + TryFrom<isize> + Debug + Copy + Clone + Send + Sync + 'static
{
}

/// Socket send operation flags.
///
/// See <https://linux.die.net/man/2/sendmsg> for more.
#[derive(Debug, Copy, Clone)]
pub enum SendFlag {
    /// Got a successful reply.
    ///
    /// Only valid for datagram and raw sockets.
    /// Only valid for IPv4 and IPv6.
    Confirm = bindings::MSG_CONFIRM as isize,

    /// Don't use a gateway to send out the packet.
    DontRoute = bindings::MSG_DONTROUTE as isize,

    /// Enables nonblocking operation.
    ///
    /// If the operation would block, return immediately with an error.
    DontWait = bindings::MSG_DONTWAIT as isize,

    /// Terminates a record.
    EOR = bindings::MSG_EOR as isize,

    /// More data will be sent.
    ///
    /// Only valid for TCP and UDP sockets.
    More = bindings::MSG_MORE as isize,

    /// Don't send SIGPIPE error if the socket is shut down.
    NoSignal = bindings::MSG_NOSIGNAL as isize,

    /// Send out-of-band data on supported sockets.
    OOB = bindings::MSG_OOB as isize,
}

impl From<SendFlag> for isize {
    fn from(value: SendFlag) -> Self {
        value as isize
    }
}

impl TryFrom<isize> for SendFlag {
    type Error = ();

    fn try_from(value: isize) -> Result<SendFlag, Self::Error> {
        let val = value as u32;
        match val {
            bindings::MSG_CONFIRM => Ok(SendFlag::Confirm),
            bindings::MSG_DONTROUTE => Ok(SendFlag::DontRoute),
            bindings::MSG_DONTWAIT => Ok(SendFlag::DontWait),
            bindings::MSG_EOR => Ok(SendFlag::EOR),
            bindings::MSG_MORE => Ok(SendFlag::More),
            bindings::MSG_NOSIGNAL => Ok(SendFlag::NoSignal),
            bindings::MSG_OOB => Ok(SendFlag::OOB),
            _ => Err(()),
        }
    }
}

impl Flag for SendFlag {}

/// Socket receive operation flags.
///
/// See <https://linux.die.net/man/2/recvmsg> for more.
#[derive(Debug, Copy, Clone)]
pub enum ReceiveFlag {
    /// Enables nonblocking operation.
    ///
    /// If the operation would block, return immediately with an error.
    DontWait = bindings::MSG_DONTWAIT as isize,

    /// Specifies that queued errors should be received from the socket error queue.
    ErrQueue = bindings::MSG_ERRQUEUE as isize,

    /// Enables out-of-band reception.
    OOB = bindings::MSG_OOB as isize,

    /// Peeks at an incoming message.
    ///
    /// The data is treated as unread and the next recv() or similar function shall still return this data.
    Peek = bindings::MSG_PEEK as isize,

    /// Returns the real length of the packet, even when it was longer than the passed buffer.
    ///
    /// Only valid for raw, datagram, netlink and UNIX datagram sockets.
    Trunc = bindings::MSG_TRUNC as isize,

    /// Waits for the full request to be satisfied.
    WaitAll = bindings::MSG_WAITALL as isize,
}

impl From<ReceiveFlag> for isize {
    fn from(value: ReceiveFlag) -> Self {
        value as isize
    }
}

impl TryFrom<isize> for ReceiveFlag {
    type Error = ();

    fn try_from(value: isize) -> Result<Self, Self::Error> {
        let val = value as u32;
        match val {
            bindings::MSG_DONTWAIT => Ok(ReceiveFlag::DontWait),
            bindings::MSG_ERRQUEUE => Ok(ReceiveFlag::ErrQueue),
            bindings::MSG_OOB => Ok(ReceiveFlag::OOB),
            bindings::MSG_PEEK => Ok(ReceiveFlag::Peek),
            bindings::MSG_TRUNC => Ok(ReceiveFlag::Trunc),
            bindings::MSG_WAITALL => Ok(ReceiveFlag::WaitAll),
            _ => Err(()),
        }
    }
}

impl Flag for ReceiveFlag {}

/// Socket `flags` field flags.
///
/// These flags are used internally by the kernel.
/// However, they are exposed here for completeness.
///
/// This enum does not implement the `Flag` trait, since it is not actually a flag.
/// Flags are often defined as a mask that can be used to retrieve the flag value; the socket flags,
/// instead, are defined as the index of the bit that they occupy in the `flags` field.
/// This means that they cannot be used as a mask, just like all the other flags that implement `Flag` do.
///
/// For example, SOCK_PASSCRED has value 3, meaning that it is represented by the 3rd bit of the `flags` field;
/// a normal flag would represent it as a mask, i.e. 1 << 3 = 0b1000.
///
/// See [include/linux/net.h](../../../../include/linux/net.h) for more.
pub enum SocketFlag {
    /// Undocumented.
    NoSpace = bindings::SOCK_NOSPACE as isize,
    /// Undocumented.
    PassCred = bindings::SOCK_PASSCRED as isize,
    /// Undocumented.
    PassSecurity = bindings::SOCK_PASSSEC as isize,
    /// Undocumented.
    SupportZeroCopy = bindings::SOCK_SUPPORT_ZC as isize,
    /// Undocumented.
    CustomSockOpt = bindings::SOCK_CUSTOM_SOCKOPT as isize,
    /// Undocumented.
    PassPidFd = bindings::SOCK_PASSPIDFD as isize,
}

impl From<SocketFlag> for isize {
    fn from(value: SocketFlag) -> Self {
        value as isize
    }
}

impl TryFrom<isize> for SocketFlag {
    type Error = ();

    fn try_from(value: isize) -> Result<Self, Self::Error> {
        let val = value as u32;
        match val {
            bindings::SOCK_NOSPACE => Ok(SocketFlag::NoSpace),
            bindings::SOCK_PASSCRED => Ok(SocketFlag::PassCred),
            bindings::SOCK_PASSSEC => Ok(SocketFlag::PassSecurity),
            bindings::SOCK_SUPPORT_ZC => Ok(SocketFlag::SupportZeroCopy),
            bindings::SOCK_CUSTOM_SOCKOPT => Ok(SocketFlag::CustomSockOpt),
            bindings::SOCK_PASSPIDFD => Ok(SocketFlag::PassPidFd),
            _ => Err(()),
        }
    }
}

/// Flags associated with a received message.
///
/// Represents the flag contained in the `msg_flags` field of a `msghdr` struct.
#[derive(Debug, Copy, Clone)]
pub enum MessageFlag {
    /// End of record.
    Eor = bindings::MSG_EOR as isize,
    /// Trailing portion of the message is discarded.
    Trunc = bindings::MSG_TRUNC as isize,
    /// Control data was discarded due to lack of space.
    Ctrunc = bindings::MSG_CTRUNC as isize,
    /// Out-of-band data was received.
    Oob = bindings::MSG_OOB as isize,
    /// An error was received instead of data.
    ErrQueue = bindings::MSG_ERRQUEUE as isize,
}

impl From<MessageFlag> for isize {
    fn from(value: MessageFlag) -> Self {
        value as isize
    }
}

impl TryFrom<isize> for MessageFlag {
    type Error = ();

    fn try_from(value: isize) -> Result<Self, Self::Error> {
        let val = value as u32;
        match val {
            bindings::MSG_EOR => Ok(MessageFlag::Eor),
            bindings::MSG_TRUNC => Ok(MessageFlag::Trunc),
            bindings::MSG_CTRUNC => Ok(MessageFlag::Ctrunc),
            bindings::MSG_OOB => Ok(MessageFlag::Oob),
            bindings::MSG_ERRQUEUE => Ok(MessageFlag::ErrQueue),
            _ => Err(()),
        }
    }
}

impl Flag for MessageFlag {}

/// Structure representing a set of flags.
///
/// This structure is used to represent a set of flags, such as the flags passed to `send` or `recv`.
/// It is generic over the type of flag that it contains.
///
/// # Invariants
/// The value of the flags must be a valid combination of the flags that it contains.
///
/// This means that the value must be the bitwise OR of the values of the flags, and that it
/// must be possible to retrieve the value of the flags from the value.
///
/// # Example
/// ```
/// use kernel::net::socket::flags::{SendFlag, FlagSet};
///
/// let mut flags = FlagSet::<SendFlag>::empty();
/// flags.insert(SendFlag::DontWait);
/// flags.insert(SendFlag::More);
/// assert!(flags.contains(SendFlag::DontWait));
/// assert!(flags.contains(SendFlag::More));
/// flags.clear();
/// assert_eq!(flags.value(), 0);
///
/// flags = FlagSet::<SendFlag>::from(SendFlag::More);
/// flags |= SendFlag::DontWait;
/// assert!(flags.contains(SendFlag::DontWait));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlagSet<T: Flag> {
    value: isize,
    _phantom: core::marker::PhantomData<T>,
}

impl<T: Flag> FlagSet<T> {
    /// Create a new empty set of flags.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::flags::{SendFlag, FlagSet};
    ///
    /// let flags = FlagSet::<SendFlag>::empty();
    /// assert_eq!(flags.value(), 0);
    /// ```
    pub fn empty() -> Self {
        FlagSet {
            value: 0,
            _phantom: core::marker::PhantomData,
        }
    }

    /// Clear all the flags set.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::flags::{SendFlag, FlagSet};
    ///
    /// let mut flags = FlagSet::<SendFlag>::from(SendFlag::More);
    /// assert!(flags.contains(SendFlag::More));
    /// flags.clear();
    /// assert_eq!(flags.value(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.value = 0;
    }

    /// Add a flag to the set.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::flags::{SendFlag, FlagSet};
    ///
    /// let mut flags = FlagSet::<SendFlag>::empty();
    /// assert!(!flags.contains(SendFlag::DontWait));
    /// flags.insert(SendFlag::DontWait);
    /// assert!(flags.contains(SendFlag::DontWait));
    /// ```
    pub fn insert(&mut self, flag: T) {
        self.value |= flag.into();
    }

    /// Remove a flag from the set.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::flags::{SendFlag, FlagSet};
    ///
    /// let mut flags = FlagSet::<SendFlag>::from(SendFlag::DontWait);
    /// assert!(flags.contains(SendFlag::DontWait));
    /// flags.remove(SendFlag::DontWait);
    /// assert!(!flags.contains(SendFlag::DontWait));
    /// ```
    pub fn remove(&mut self, flag: T) {
        self.value &= !flag.into();
    }

    /// Check if a flag is set.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::flags::{SendFlag, FlagSet};
    ///
    /// let mut flags = FlagSet::<SendFlag>::from(SendFlag::DontWait);
    /// assert!(flags.contains(SendFlag::DontWait));
    /// ```
    pub fn contains(&self, flag: T) -> bool {
        self.value & flag.into() != 0
    }

    /// Get the integer value of the flags set.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::flags::{SendFlag, FlagSet};
    ///
    /// let flags = FlagSet::<SendFlag>::from(SendFlag::DontWait);
    /// assert_eq!(flags.value(), SendFlag::DontWait as isize);
    /// ```
    pub fn value(&self) -> isize {
        self.value
    }
}

impl<T: Flag> BitOr<T> for FlagSet<T> {
    type Output = FlagSet<T>;

    fn bitor(self, rhs: T) -> Self::Output {
        FlagSet {
            value: self.value | rhs.into(),
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<T: Flag> BitOrAssign<T> for FlagSet<T> {
    fn bitor_assign(&mut self, rhs: T) {
        self.value |= rhs.into();
    }
}

// impl from isize for any flags<T>
impl<T: Flag> From<isize> for FlagSet<T> {
    fn from(value: isize) -> Self {
        FlagSet {
            value,
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<T: Flag> From<T> for FlagSet<T> {
    fn from(value: T) -> Self {
        Self::from(value.into())
    }
}

impl<T: Flag> FromIterator<T> for FlagSet<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut flags = FlagSet::empty();
        for flag in iter {
            flags.insert(flag);
        }
        flags
    }
}

impl<T: Flag> From<FlagSet<T>> for isize {
    fn from(value: FlagSet<T>) -> Self {
        value.value
    }
}

impl<T: Flag> IntoIterator for FlagSet<T> {
    type Item = T;
    type IntoIter = FlagSetIterator<T>;

    fn into_iter(self) -> Self::IntoIter {
        FlagSetIterator {
            flags: self,
            current: 0,
        }
    }
}

/// Iterator over the flags in a set.
///
/// This iterator iterates over the flags in a set, in order of increasing value.
///
/// # Example
/// ```
/// use kernel::net::socket::flags::{SendFlag, FlagSet};
///
/// let mut flags = FlagSet::from_iter([SendFlag::DontWait, SendFlag::More]);
/// for flag in flags.into_iter() {
///    println!("Flag: {:?}", flag);
/// }
/// ```
pub struct FlagSetIterator<T: Flag> {
    flags: FlagSet<T>,
    current: usize,
}

impl<T: Flag> Iterator for FlagSetIterator<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let mut value = 1 << self.current;
        while value <= self.flags.value {
            self.current += 1;
            if self.flags.value & value != 0 {
                if let Ok(flag) = T::try_from(value) {
                    return Some(flag);
                }
            }
            value = 1 << self.current;
        }
        None
    }
}

/// Create a set of flags from a list of flags.
///
/// This macro provides a compact way to create empty sets and sets from a list of flags.
///
/// # Example
/// ```
/// use kernel::net::socket::flags::SendFlag;
/// use kernel::flag_set;
///
/// let mut flags = flag_set!(SendFlag::DontWait, SendFlag::More);
/// assert!(flags.contains(SendFlag::DontWait));
/// assert!(flags.contains(SendFlag::More));
///
/// let mut empty_flags = flag_set!();
/// assert_eq!(empty_flags.value(), 0);
/// ```
#[macro_export]
macro_rules! flag_set {
    () => {
        $crate::net::socket::flags::FlagSet::empty()
    };
    ($($flag:expr),+) => {
        $crate::net::socket::flags::FlagSet::from_iter([$($flag),+])
    };
}
