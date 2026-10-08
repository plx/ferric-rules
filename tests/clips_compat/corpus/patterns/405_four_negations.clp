(deffacts d (a))
(defrule r (not (not (not (not (a))))) => (printout t four crlf))
