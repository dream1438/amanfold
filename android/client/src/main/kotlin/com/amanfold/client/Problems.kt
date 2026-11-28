package com.amanfold.client

import com.amanfold.core.AmanfoldException

/** What went wrong, in terms the interface has a sentence for. */
enum class Problem {
    WrongPassword,
    SecretKey,
    RecoveryWords,
    Offline,
    Exists,
    BadServer,
    ServerKey,
    Rollback,
    SignedOut,
    Verification,
    Code,
    RateLimited,
    Locked,
    ReadOnly,
    SafetyNumber,
    Unverified,
    NotFound,
    Server,
    Other,
}

/**
 * Maps any failure from the client or the core to a [Problem]. [hadCode] is
 * true when the call carried an emailed code, so a refusal most likely means
 * the code was wrong.
 */
fun problemOf(e: Throwable, hadCode: Boolean = false): Problem = when (e) {
    is LockedException -> Problem.Locked
    is ReadOnlyException -> Problem.ReadOnly
    is FingerprintChangedException -> Problem.SafetyNumber
    is UnverifiedMembersException -> Problem.Unverified
    is ServerKeyException -> Problem.ServerKey
    is SecretKeyException -> Problem.SecretKey
    is BadServerException -> Problem.BadServer
    is AmanfoldException.Decrypt -> Problem.WrongPassword
    is AmanfoldException.InvalidCode ->
        if (e.reason.contains("secret", ignoreCase = true)) Problem.SecretKey else Problem.RecoveryWords
    is AmanfoldException.Rollback, is AmanfoldException.BadSignature -> Problem.Rollback
    is ApiException -> when {
        e.status == 0 -> Problem.Offline
        e.kind == "exists" -> Problem.Exists
        e.kind == "rate_limited" -> Problem.RateLimited
        hadCode && e.kind == "verification_failed" -> Problem.Code
        e.kind == "verification_failed" -> Problem.Verification
        e.status == 401 -> Problem.SignedOut
        e.status == 404 -> Problem.NotFound
        else -> Problem.Server
    }
    else -> Problem.Other
}
