(deffacts d (key 1) (key 2) (key 3) (b 1) (c 2))
(defrule r (key ?x) (not (or (b ?x) (c ?x))) => (printout t "clear " ?x crlf))
