;; Phase 2 chain reaction fixture
;; Tests: assert actions triggering subsequent rule activations
(deffacts startup
    (input data))

(defrule step1
    (input ?x)
    =>
    (assert (stage1 ?x)))

(defrule step2
    (stage1 ?x)
    =>
    (assert (stage2 ?x)))

(defrule step3
    (stage2 ?x)
    =>
    (assert (complete ?x)))
