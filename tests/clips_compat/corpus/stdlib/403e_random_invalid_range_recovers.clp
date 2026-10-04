(defrule run =>
  (seed 42)
  (printout t "value:" (random 5 2) ";next:" (random) crlf))
