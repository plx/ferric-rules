(defrule symbols =>
  (printout t (fact-existp x) " " (fact-relation x) " " (fact-slot-names x) " " (fact-slot-value x y) crlf)
  (printout t continued crlf))
