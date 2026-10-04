(defrule run =>
  (bind ?now (time))
  (printout t (floatp ?now) ":" (> ?now 0) crlf))
