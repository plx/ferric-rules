(defrule run =>
  (seed 42)
  (printout t (random) " " (random) " " (random) crlf)
  (seed 42)
  (printout t (random) " " (random) " " (random) crlf))
