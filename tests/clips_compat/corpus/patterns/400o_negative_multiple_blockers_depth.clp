(deffacts seed (item 1) (item 2) (item 3) (blocker first) (blocker second))
(defrule clear-first (declare (salience 20)) ?b <- (blocker first) =>
  (retract ?b) (printout t cleared-first crlf))
(defrule clear-last (declare (salience 10)) ?b <- (blocker second) =>
  (retract ?b) (printout t cleared-last crlf))
(defrule r (item ?x) (not (blocker ?)) => (printout t "r " ?x crlf))
