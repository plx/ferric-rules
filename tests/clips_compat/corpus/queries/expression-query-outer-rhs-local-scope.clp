
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
  (bind ?f 42)
  (bind ?minimum 15)
  (bind ?found (find-all-facts ((?f item)) (> ?f:value ?minimum)))
  (printout t (length$ ?found) ":" ?f ":" ?minimum crlf))
