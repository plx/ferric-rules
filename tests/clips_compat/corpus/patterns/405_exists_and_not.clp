(deffacts d (item 1) (item 2) (done 1) (phase 0))
(defrule ex (declare (salience 10)) (item ?x) (exists (and (not (done ?x)))) => (printout t "open " ?x crlf))
(defrule finish ?p <- (phase 0) => (retract ?p) (assert (done 2) (phase 1)) (printout t finish crlf))
(defrule reopen ?p <- (phase 1) ?f <- (done 1) => (retract ?p ?f) (printout t reopen crlf))
