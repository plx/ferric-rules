(deffunction lower () (printout t "low:" (random) ";") 0)
(deffunction upper () (printout t "high:" (random) ";") 100)
(defrule run =>
  (seed 42)
  (printout t "value:" (random (lower) (upper)) ";next:" (random) crlf))
