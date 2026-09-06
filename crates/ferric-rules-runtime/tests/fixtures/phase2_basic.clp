;; Phase 2 basic integration fixture
;; Tests: deftemplate, deffacts, defrule, assert action, run
(deftemplate person
    (slot name)
    (slot age))

(deffacts startup
    (person (name Alice) (age 30))
    (person (name Bob) (age 25)))

(defrule greet-person
    (person (name ?name) (age ?age))
    =>
    (assert (greeted ?name)))
