(deffunction mark () (printout t skipped) 1)
(defrule run => (seed 42) (printout t "value:" (random (mark)) ";next:" (random) crlf))
