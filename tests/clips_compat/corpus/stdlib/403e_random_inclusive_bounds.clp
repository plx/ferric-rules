(defrule run =>
  (seed 42)
  (printout t (random 0 10) " " (random -5 5) " " (random 7 7) " " (random) crlf)
  (seed 42)
  (printout t (random -9223372036854775808 9223372036854775807) crlf))
