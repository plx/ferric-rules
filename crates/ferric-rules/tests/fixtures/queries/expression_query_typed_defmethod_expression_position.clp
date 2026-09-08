
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defgeneric matching)
(defmethod matching ((?minimum INTEGER))
  (find-all-facts ((?f item)) (> ?f:value ?minimum)))
(defrule probe =>
  (printout t (length$ (matching 15)) ":"
    (fact-slot-value (nth$ 1 (matching 15)) value) crlf))
