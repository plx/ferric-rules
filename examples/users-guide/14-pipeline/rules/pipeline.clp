;;; ============================================================
;;; A worked example pulling together the moving parts: templates
;;; for the input shape, a deffunction helper, a defglobal tuning
;;; knob, and salience-ordered rule phases (normalize → diagnose).
;;; Single-module to keep within ferric's current limits on
;;; cross-module rule chaining.
;;; ============================================================

(defglobal ?*scale* = 1.0)

(deftemplate reading
    (slot id)
    (slot kind)
    (slot value))

;;; Phase 1 — normalize: rewrite Fahrenheit readings to Celsius.
(deffunction f-to-c (?f) (/ (- ?f 32.0) 1.8))

(defrule normalize
    (declare (salience 100))
    ?r <- (reading (kind fahrenheit) (value ?v))
    =>
    (modify ?r (kind celsius) (value (* ?*scale* (f-to-c ?v)))))

;;; Phase 2 — diagnose: classify each Celsius reading.
;;; Diagnoses come out as ordered facts: (diagnosis <id> <level> "<message>").

(defrule overheat
    (declare (salience 50))
    (reading (id ?i) (kind celsius) (value ?c))
    (test (> ?c 40))
    =>
    (assert (diagnosis ?i alert "overheat")))

(defrule nominal
    (declare (salience 50))
    (reading (id ?i) (kind celsius) (value ?c))
    (test (<= ?c 40))
    =>
    (assert (diagnosis ?i info "nominal")))
