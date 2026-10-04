(deffacts d )
(defrule r (exists (not (a))) => (printout t "exists_not" crlf))
