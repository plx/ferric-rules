
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(deffunction matching (?minimum)
  (find-all-facts ((?f item)) (> ?f:value ?minimum)))
(deffunction has-match (?minimum)
  (any-factp ((?f item)) (> ?f:value ?minimum)))
(defrule probe =>
  (printout t (length$ (matching 10)) ":" (has-match 40) ":"
    (fact-slot-value (nth$ 1 (matching 10)) value) crlf))
