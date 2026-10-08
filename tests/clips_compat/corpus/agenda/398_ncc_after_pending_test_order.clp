(deffacts d (item 1))
(defrule b (item ?x) (not (and (blocker) (other))) => (printout t b crlf))
(defrule a (item ?x) (test (> ?x 0)) => (printout t a crlf))
