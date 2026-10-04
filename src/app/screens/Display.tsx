import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ThumbsDown, ThumbsUp } from "@untitledui/icons";
import { Button } from "@/components/base/buttons/button";
import { formatMoney } from "@/app/format";
import { DISPLAY_EVENT, type DisplayPayload } from "@/app/display-channel";
import { submitRating } from "@/app/ipc";

const IDLE: DisplayPayload = {
    state: "idle",
    lines: [],
    subtotal: "0",
    discountTotal: "0",
    taxTotal: "0",
    grandTotal: "0",
    savedTotal: "0",
    paymentMethod: null,
    paidTotal: null,
    changeDue: null,
    saleId: null,
    loyaltyEarned: 0,
    touch: false,
};

/**
 * The customer mirror: a read-only second window facing the shopper.
 *
 * Three live states follow the till — idle welcome, the cart as it rings, the
 * thank-you with change. The only input anywhere is the rating tap on the
 * thank-you screen, and only when the till has touch enabled: one tap, then the
 * screen thanks and goes quiet until the next sale.
 *
 * Big type, high contrast, no navigation — a shopper reads this from across the
 * counter, and there is nothing here to tap through to.
 */
export const Display = () => {
    const [view, setView] = useState<DisplayPayload>(IDLE);
    const [rated, setRated] = useState(false);
    const [rateError, setRateError] = useState<string | null>(null);

    useEffect(() => {
        let unlisten: (() => void) | null = null;
        let timer: ReturnType<typeof setTimeout> | null = null;

        const armIdleFallback = () => {
            if (timer !== null) clearTimeout(timer);
            // A thank-you left standing when the till goes quiet reverts on its
            // own, so a forgotten sale never blocks the welcome screen.
            timer = setTimeout(() => {
                setView(IDLE);
                setRated(false);
                setRateError(null);
            }, 90_000);
        };

        listen<DisplayPayload>(DISPLAY_EVENT, (event) => {
            setView(event.payload);
            setRated(false);
            setRateError(null);
            armIdleFallback();
        })
            .then((off) => {
                unlisten = off;
            })
            .catch(() => {
                // No bridge outside the Tauri window (dev server, tests) — the
                // welcome screen stands in until events arrive.
            });
        armIdleFallback();

        return () => {
            unlisten?.();
            if (timer !== null) clearTimeout(timer);
        };
    }, []);

    const rate = async (rating: "Like" | "Dislike") => {
        if (rated) return;
        setRateError(null);
        try {
            await submitRating(view.saleId, rating);
            setRated(true);
        } catch {
            // A failed tap must not strand the screen on an error the shopper
            // cannot act on — the thank-you stands, the tap is simply lost.
            setRateError("Peringkat tidak tersimpan — terima kasih atas kunjungan Anda.");
            setRated(true);
        }
    };

    if (view.state === "idle") {
        return (
            <div className="flex h-dvh flex-col items-center justify-center gap-4 bg-primary p-8 text-center">
                <p className="text-display-xl font-semibold text-primary">Selamat datang</p>
                <p className="text-xl text-tertiary">Terima kasih telah berbelanja di sini</p>
            </div>
        );
    }

    if (view.state === "done") {
        return (
            <div className="flex h-dvh flex-col items-center justify-center gap-6 bg-primary p-8 text-center">
                <p className="text-display-xl font-semibold text-primary">Terima kasih</p>
                <p className="text-display-md font-semibold tabular-nums text-primary">
                    {formatMoney(view.grandTotal)}
                </p>
                {view.paymentMethod && (
                    <p className="text-xl text-tertiary">
                        {view.paymentMethod}
                        {view.changeDue !== null && view.changeDue !== "0" && view.changeDue !== "0.000"
                            ? ` · Kembalian ${formatMoney(view.changeDue)}`
                            : ""}
                    </p>
                )}
                {view.loyaltyEarned > 0 && (
                    <p className="text-xl text-tertiary">+{view.loyaltyEarned} poin loyalty</p>
                )}
                {view.touch && !rated && (
                    <div className="flex flex-col items-center gap-3">
                        <p className="text-xl text-secondary">Puas dengan pelayanan kami?</p>
                        <div className="flex gap-4">
                            <Button
                                size="xl"
                                color="primary"
                                iconLeading={ThumbsUp}
                                aria-label="Puas"
                                onPress={() => void rate("Like")}
                                className="px-10 py-6 text-2xl"
                            >
                                Puas
                            </Button>
                            <Button
                                size="xl"
                                color="secondary"
                                iconLeading={ThumbsDown}
                                aria-label="Tidak puas"
                                onPress={() => void rate("Dislike")}
                                className="px-10 py-6 text-2xl"
                            >
                                Kurang
                            </Button>
                        </div>
                    </div>
                )}
                {rated && (
                    <p className="text-xl text-tertiary">
                        {rateError ?? "Terima kasih atas penilaian Anda."}
                    </p>
                )}
            </div>
        );
    }

    return (
        <div className="flex h-dvh flex-col bg-primary p-8">
            <p className="text-display-md font-semibold text-primary">Belanja Anda</p>
            <div className="flex flex-1 flex-col gap-2 overflow-hidden py-4">
                {view.lines.map((line, index) => (
                    <div key={index} className="flex items-baseline justify-between gap-4">
                        <p className="truncate text-2xl text-primary">
                            {line.name}
                            <span className="text-tertiary"> × {line.quantity}</span>
                        </p>
                        <p className="shrink-0 text-2xl tabular-nums text-primary">{formatMoney(line.total)}</p>
                    </div>
                ))}
                {view.lines.length === 0 && (
                    <p className="text-2xl text-tertiary">Item muncul di sini saat dipindai…</p>
                )}
            </div>
            <div className="flex flex-col gap-1 border-t border-secondary pt-4">
                <div className="flex justify-between text-xl text-tertiary">
                    <span>Subtotal</span>
                    <span className="tabular-nums">{formatMoney(view.subtotal)}</span>
                </div>
                {view.discountTotal !== "0" && view.discountTotal !== "0.000" && (
                    <div className="flex justify-between text-xl text-tertiary">
                        <span>Diskon</span>
                        <span className="tabular-nums">−{formatMoney(view.discountTotal)}</span>
                    </div>
                )}
                {view.savedTotal !== "0" && view.savedTotal !== "0.000" && (
                    <div className="flex justify-between text-xl font-medium text-success-primary">
                        <span>Anda hemat</span>
                        <span className="tabular-nums">{formatMoney(view.savedTotal)}</span>
                    </div>
                )}
                <div className="flex justify-between pt-2">
                    <span className="text-display-sm font-semibold text-primary">Total</span>
                    <span className="text-display-sm font-semibold tabular-nums text-primary">
                        {formatMoney(view.grandTotal)}
                    </span>
                </div>
                {view.state === "payment" && view.paymentMethod && (
                    <p className="pt-2 text-center text-2xl text-tertiary">
                        {view.paymentMethod} · {formatMoney(view.grandTotal)}
                    </p>
                )}
            </div>
        </div>
    );
};
