(defrule run =>
  (seed 4294967296)
  (printout t (random) " " (random) crlf)
  (seed 0)
  (printout t (random) " " (random) crlf)
  (seed 2147483648)
  (printout t (random) " " (random) crlf)
  (printout t "seed:" (seed -1) ":" (random) ":" (random) crlf))
