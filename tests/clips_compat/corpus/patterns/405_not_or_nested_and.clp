(deffacts d (key 1) (key 2) (key 3) (b 1) (c 1) (d 2))
(defrule r (key ?x) (not (or (and (b ?x) (c ?x)) (d ?x))) => (printout t "clear " ?x crlf))
