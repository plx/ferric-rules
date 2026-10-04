(defrule run =>
  (seed 42)
  (printout t (random 0 10) " " (random -5 5) " " (random 7 7) " " (random) crlf)
  (seed 42)
  ; Keep the inclusive width at 2^31: CLIPS computes it in signed long long.
  ; The full i64 span overflows in the reference; this still tests its minimum.
  (printout t (random -9223372036854775808 -9223372034707292161) crlf))
