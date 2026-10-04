(deffacts d (a 1) (a 2) (limit 0) (limit 2))
(defrule ft (limit ?minimum) (forall (a ?x) (test (>= ?x ?minimum))) =>
 (printout t "minimum " ?minimum crlf))
