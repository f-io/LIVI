use crate::csm_message;

csm_message! {
    pub struct RequestAuthenticationCertificate = 0xAA00 {}
}

csm_message! {
    pub struct AuthenticationCertificate = 0xAA01 {
        0 => certificate: [bytes],
    }
}

csm_message! {
    pub struct RequestAuthenticationChallengeResponse = 0xAA02 {
        0 => challenge: [bytes],
    }
}

csm_message! {
    pub struct AuthenticationResponse = 0xAA03 {
        0 => response: [bytes],
    }
}

csm_message! {
    pub struct AuthenticationFailed = 0xAA04 {}
}

csm_message! {
    pub struct AuthenticationSucceeded = 0xAA05 {}
}
