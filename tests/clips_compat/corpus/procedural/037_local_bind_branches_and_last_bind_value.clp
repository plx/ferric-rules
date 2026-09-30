(deffunction choose (?flag)
  (if ?flag then (bind ?answer 10) else (bind ?answer 20))
  (bind ?answer (+ ?answer 1)))
(defrule probe => (printout t (choose TRUE) ":" (choose FALSE) crlf))
