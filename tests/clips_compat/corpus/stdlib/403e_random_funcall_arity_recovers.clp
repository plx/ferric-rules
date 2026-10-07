(deffunction mark () (printout t marker) 1)
(defrule run =>
  (seed 42)
  (printout t "value:" (funcall random (mark) 2 3) ";next:" (random) crlf))
