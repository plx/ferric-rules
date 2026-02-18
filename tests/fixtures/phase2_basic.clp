;; Phase 2 basic integration fixture
;; Tests: deftemplate, deffacts, defrule, assert action, run
(deftemplate person
    (slot name)
    (slot age))

(deffacts startup
    (person Alice 30)
    (person Bob 25))

(defrule greet-person
    (person ?name ?age)
    =>
    (assert (greeted ?name)))
