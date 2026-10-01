(deffunction missing () (bind ?local 10) (bind ?local) ?local)
(defrule probe => (printout t (missing) crlf))
