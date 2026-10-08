(deffacts d (item 1) (item 2) (done 2))
(defrule hit (item ?x) (not (and (item ?x) (exists (done ?x)))) => (printout t "open " ?x crlf))
