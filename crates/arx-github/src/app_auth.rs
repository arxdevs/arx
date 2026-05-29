//! GitHub App authentication: minting the short-lived JWT that authenticates
//! arx *as the App itself*. This JWT is required for the `/app/*` endpoints
//! (listing installations, minting installation tokens, updating the webhook
//! config) — see [`crate::api`].

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

/// GitHub caps an App JWT's lifetime at 10 minutes. We stay just under it.
const JWT_TTL_SECS: u64 = 9 * 60;
/// Backdate `iat` to tolerate clock drift between us and GitHub.
const CLOCK_SKEW_SECS: u64 = 60;

#[derive(Serialize)]
struct AppClaims {
    iat: u64,
    exp: u64,
    iss: i64,
}

/// Mints a short-lived RS256 JWT signed with the App's private key, with the
/// App id as issuer. Valid for ~9 minutes.
pub fn app_jwt(app_id: i64, private_key_pem: &str) -> Result<String, arx_core::Error> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| arx_core::Error::Internal(format!("clock before unix epoch: {e}")))?
        .as_secs();

    let claims = AppClaims {
        iat: now.saturating_sub(CLOCK_SKEW_SECS),
        exp: now + JWT_TTL_SECS,
        iss: app_id,
    };

    let key = jsonwebtoken::EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
        .map_err(|e| arx_core::Error::Internal(format!("parse github app private key: {e}")))?;
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &key,
    )
    .map_err(|e| arx_core::Error::Internal(format!("sign github app jwt: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Throwaway 2048-bit RSA keypair generated only for this test. Not a secret.
    const TEST_PRIVATE_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEArZU0mpTuZ2mBgc3g7+c+F/pp3nBaeR2kSDl0XHzAzoBC2vvP
Lbzc2QBmA9pK4XtnS6ZunLkUBzhhqJJ5zNTmF88jAxgKE/PC30ORWlLboTmLewdS
fOaHgRuOeIX2uni3UkGlj6RUxyGtV9jr8AMCPFZgQI227vKK8/kRwwttNdNChmIU
Emohf1awitI+NskPqEaN0I0kl+Q7gma2dTa8tIwWePeGy1pdJeIdrIv8k04xRF2/
UBekWVf6tSDfk2lb161YrDGNNXOQ6tgBMzddudNu6/4dCSFcUkt/LORwKjYB04H/
7pbGhAju06cIAx9xot16rSEyE/qAfTA2S6grbQIDAQABAoIBAAL/jMUOxX9rxxzi
3XvHVr87SBDbh/SHmorU0zm1ve7TMFRv/QghNv9YjmqKnrh+VS5tVYPHfp0RUD6F
KS1sj/zhSw2GoMXvc/I/TIdu3vRN9ibN3ZLiuHx2aWOTjMtzwlbdY2qzv/Mglcnq
qUigBK3eIBN9XyeJcPT93FyuGdjQIgskuPdOQiHADo3sE7fOu4R8kJi3M8y7dtVp
8KBtna5ho4BLOTRzlqgkdLMJ+clrq86WImI1YPcMvy51W+hMWLDyaXBDA1cpcNuY
m7hEqys0DuATuKNRKiKX41xnLO40xfk+5R1FrX7Rgapr858q3tmDvHA4xyi8F2n5
Tec4fFECgYEA5VJvU/ZNQnRgadYmWx7uE7zVgpJCjLbmMeWkCM2gQBoak1i1Twd7
5ynmPJnqUjFerE+fnRBODsxKuVMyjxDAyCalCu65Cmzk3iMe+Y788uKdaGMKYv9U
3ibj36I7tKj/d3sliKNiEcRY3HQvcq+X5XcYX1feTG5PCnW7oa2ZKUcCgYEAwcbF
fvkCUB12qNvg4x6JuSgH3blckY++0GykXYPNMJ43MUaKPxNLNrk+oPX78Cw8IdhC
48EytDXS21fbsAXEZUPHib5F8Y58mh8jBaJUxyNfKrAqvsdbFxnCi5MhpXtmcXbr
wz3o/7fFrCuKHh5/2siVoFFeLMSYblNHYO/WH6sCgYEAydcDu9/24n3x+lWNzvfr
Tp4PQuye/KFi/RoFheYOnT0clQIoGxYYPT+IsWA7ePqRPJKchy65tZakUnfi8T8q
n0A8VeIGJiHwU4CQG9F53AIPz7gUhUv9E8chHE37xShWKoDOaXR8teye1fLBbG0X
AdYQMqLxO05/7VHwwv4757MCgYEAiQfQl+btfMwpImZDVTk+OYKWdXRkgsc9L9T0
MvFGxE/ORflVQB+bu7oqENeC7yfI6kItozP3cDrzvosV3xdk+BuDWuQEQDr74F2O
fah6/UwFO4HS6JC/2Mktq1hDnety2WA4fxwjzdoeXo93n67/yS65qOKBj3UDOlmI
C4PvTvsCgYBTLi2svMGbJRrHyKuoeWBKWya90A38xVlWTHGSDjzA8ocr38oKp97B
UWPfGLdYDJlh5uNd/+lSYe+L1f1CtxYFhizQdqQ50lxi3oItaWtfsL4jbjkZCHlK
rOUNUckXBQ7NM+FmfZiV1e9t/w1rt/s0p5YfUqkiPkzLkY3u5Twd7w==
-----END RSA PRIVATE KEY-----";

    const TEST_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEArZU0mpTuZ2mBgc3g7+c+
F/pp3nBaeR2kSDl0XHzAzoBC2vvPLbzc2QBmA9pK4XtnS6ZunLkUBzhhqJJ5zNTm
F88jAxgKE/PC30ORWlLboTmLewdSfOaHgRuOeIX2uni3UkGlj6RUxyGtV9jr8AMC
PFZgQI227vKK8/kRwwttNdNChmIUEmohf1awitI+NskPqEaN0I0kl+Q7gma2dTa8
tIwWePeGy1pdJeIdrIv8k04xRF2/UBekWVf6tSDfk2lb161YrDGNNXOQ6tgBMzdd
udNu6/4dCSFcUkt/LORwKjYB04H/7pbGhAju06cIAx9xot16rSEyE/qAfTA2S6gr
bQIDAQAB
-----END PUBLIC KEY-----";

    #[derive(serde::Deserialize)]
    struct DecodedClaims {
        iss: i64,
        iat: u64,
        exp: u64,
    }

    #[test]
    fn app_jwt_is_rs256_and_decodes_with_expected_claims() {
        let token = app_jwt(12345, TEST_PRIVATE_KEY).expect("should mint a jwt");

        let header = jsonwebtoken::decode_header(&token).unwrap();
        assert_eq!(header.alg, jsonwebtoken::Algorithm::RS256);

        let key = jsonwebtoken::DecodingKey::from_rsa_pem(TEST_PUBLIC_KEY.as_bytes()).unwrap();
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
        validation.validate_exp = true;
        validation.required_spec_claims.clear();
        let decoded = jsonwebtoken::decode::<DecodedClaims>(&token, &key, &validation)
            .expect("signature and exp should validate");

        assert_eq!(decoded.claims.iss, 12345);
        assert!(decoded.claims.exp > decoded.claims.iat);
        assert!(decoded.claims.exp - decoded.claims.iat <= JWT_TTL_SECS + CLOCK_SKEW_SECS);
    }

    #[test]
    fn app_jwt_rejects_malformed_key() {
        let err = app_jwt(1, "not a pem");
        assert!(err.is_err());
    }
}
