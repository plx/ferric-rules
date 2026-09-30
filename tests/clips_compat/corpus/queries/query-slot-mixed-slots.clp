;; Named slots resolve against each template, regardless of physical position.
(deftemplate left-item (slot padding) (slot value) (multislot parts))
(deftemplate right-item (slot value) (slot padding))
(deffacts seed
  (left-item (padding left) (value 10) (parts a b))
  (right-item (value 20) (padding right)))
(defrule probe =>
  (do-for-all-facts ((?a left-item) (?b right-item))
    (and (< ?a:value ?b:value) (= (length$ ?a:parts) 2))
    (printout t ?a:value ":" ?b:value ":" (length$ ?a:parts) ":" (implode$ ?a:parts) crlf)))
