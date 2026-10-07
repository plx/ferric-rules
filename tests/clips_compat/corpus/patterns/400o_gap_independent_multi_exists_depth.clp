(deffacts seed (item 1) (item 2) (item 3) (blocker) (other))
(defrule r (item ?x) (exists (blocker) (other)) => (printout t ?x crlf))
