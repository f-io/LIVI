from dataclasses import dataclass

from iap2.control_session_message import csm


@csm(0xAA00)
class RequestAuthenticationCertificate:
    pass


@csm(0xAA01)
@dataclass
class AuthenticationCertificate:
    certificate: bytes


@csm(0xAA02)
class RequestAuthenticationChallengeResponse:
    challenge: bytes


@csm(0xAA03)
@dataclass
class AuthenticationResponse:
    response: bytes


@csm(0xAA04)
class AuthenticationFailed:
    pass


@csm(0xAA05)
class AuthenticationSucceeded:
    pass
