(deffunction conditional (?flag)
  (if ?flag then (bind ?local 10))
  ?local)
(defrule probe => (printout t (conditional FALSE) crlf))
